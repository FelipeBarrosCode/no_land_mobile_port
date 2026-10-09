//! Darwin's unprivileged ICMP datagram socket replaces the desktop ping process.
use std::{
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    os::fd::{AsRawFd, FromRawFd},
    time::{Duration, Instant},
};

pub(super) fn probe_mtu(destination: IpAddr, packet_size: u16, count: u8) -> io::Result<bool> {
    // The managed tunnel carries only the IPv4 host route 10.77.0.1/32.
    if !destination.is_ipv4() || packet_size < 44 || count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected an IPv4 tunnel MTU probe",
        ));
    }
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // UdpSocket owns/closes the datagram descriptor; the protocol is ICMP.
    let socket = unsafe { UdpSocket::from_raw_fd(fd) };
    let enabled: libc::c_int = 1;
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_DONTFRAG,
            (&enabled as *const libc::c_int).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    socket.set_write_timeout(Some(Duration::from_secs(1)))?;
    socket.connect(SocketAddr::new(destination, 0))?;
    let nonce = uuid::Uuid::new_v4();
    let mut request = vec![0u8; usize::from(packet_size) - 20];
    request[0] = 8; // ICMP echo request
    request[8..24].copy_from_slice(nonce.as_bytes());
    let mut received = vec![0u8; usize::from(packet_size) + 64];
    for sequence in 0..u16::from(count) {
        request[2..4].fill(0);
        request[6..8].copy_from_slice(&sequence.to_be_bytes());
        let sum = checksum(&request);
        request[2..4].copy_from_slice(&sum.to_be_bytes());
        match socket.send(&request) {
            Ok(size) if size == request.len() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.raw_os_error() == Some(libc::EMSGSIZE) => return Ok(false),
            Err(error) => return Err(error),
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(false);
            };
            socket.set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
            match socket.recv(&mut received) {
                Ok(size) if matches_reply(&received[..size], &request) => break,
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(false)
                }
                Err(error) => return Err(error),
            }
        }
    }
    // All four replies are required, matching the desktop <=5% loss threshold.
    Ok(true)
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn matches_reply(packet: &[u8], request: &[u8]) -> bool {
    // Darwin may include the IPv4 header. ICMP error responses and unrelated
    // echo replies must not count as proof that this exact sized packet arrived.
    let offset = if packet.first().is_some_and(|b| b >> 4 == 4) {
        let length = usize::from(packet[0] & 15) * 4;
        if length < 20 {
            return false;
        }
        length
    } else {
        0
    };
    let Some(icmp) = packet.get(offset..) else {
        return false;
    };
    icmp.len() == request.len()
        && icmp.len() >= 24
        && icmp[0] == 0
        && icmp[1] == 0
        && icmp[6..] == request[6..]
        && checksum(icmp) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_echo_size_nonce_sequence_and_checksum() {
        let mut request = vec![0; 556];
        request[0] = 8;
        request[6..8].copy_from_slice(&9u16.to_be_bytes());
        request[8..24].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        let mut reply = request.clone();
        reply[0] = 0;
        let sum = checksum(&reply);
        reply[2..4].copy_from_slice(&sum.to_be_bytes());
        assert!(matches_reply(&reply, &request));
        let mut with_ip = vec![0u8; 20];
        with_ip[0] = 0x45;
        with_ip.extend_from_slice(&reply);
        assert!(matches_reply(&with_ip, &request));
        assert!(!matches_reply(&reply[..reply.len() - 1], &request));
        reply[8] ^= 1;
        assert!(!matches_reply(&reply, &request));
    }

    #[test]
    fn probes_darwin_loopback_without_executables() {
        assert!(probe_mtu("127.0.0.1".parse().unwrap(), 576, 4).unwrap());
        assert!(probe_mtu("127.0.0.1".parse().unwrap(), 1440, 4).unwrap());
    }
}

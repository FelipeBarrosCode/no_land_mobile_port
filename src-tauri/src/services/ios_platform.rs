use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, CStr};

unsafe extern "C" {
    fn noland_ios_clipboard_read() -> *mut c_char;
    fn noland_ios_clipboard_write(bytes: *const u8, length: usize) -> *mut c_char;
    fn noland_ios_keep_awake(active: bool);
    fn noland_ios_response_free(value: *mut c_char);
}

#[derive(Deserialize)]
struct Response {
    text: Option<String>,
    error: Option<String>,
}

fn decode(pointer: *mut c_char) -> AppResult<Response> {
    if pointer.is_null() {
        return Err(AppError::Command(
            "iOS clipboard returned no response".into(),
        ));
    }
    // Native response is malloc-owned and freed exactly once, even on decode failure.
    let response =
        unsafe { serde_json::from_slice::<Response>(CStr::from_ptr(pointer).to_bytes()) };
    unsafe {
        noland_ios_response_free(pointer);
    }
    let response = response?;
    if let Some(error) = response.error {
        return Err(AppError::Command(error));
    }
    Ok(response)
}

pub fn read_clipboard() -> AppResult<String> {
    decode(unsafe { noland_ios_clipboard_read() })?
        .text
        .ok_or_else(|| AppError::Command("iOS clipboard returned no text".into()))
}

pub fn write_clipboard(text: &str) -> AppResult<()> {
    decode(unsafe { noland_ios_clipboard_write(text.as_ptr(), text.len()) })?;
    Ok(())
}

pub fn keep_awake(active: bool) {
    unsafe {
        noland_ios_keep_awake(active);
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneStatus {
    pub captured_samples: u64,
    pub encoded_packets: u64,
    pub sent_bytes: u64,
    pub dropped_samples: u64,
    pub rtcp_reports: u64,
    pub network_errors: u64,
    pub queue_depth_samples: u32,
    pub running: bool,
    pub suspended: bool,
    pub muted: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn start_microphone(
    host: &str,
    rtp_port: u16,
    rtcp_port: u16,
    local_rtcp_port: u16,
    ssrc: u32,
    sequence_offset: u16,
    timestamp_offset: u32,
    bitrate_bps: u32,
    frame_ms: u32,
) -> AppResult<()> {
    let host = std::ffi::CString::new(host)
        .map_err(|_| AppError::InvalidInput("Microphone host contains an interior NUL".into()))?;
    let result = unsafe {
        crate::moonlight::native::nl_microphone_start(
            host.as_ptr(),
            rtp_port,
            rtcp_port,
            local_rtcp_port,
            ssrc,
            sequence_offset,
            timestamp_offset,
            bitrate_bps,
            frame_ms,
        )
    };
    match result {
        0 => Ok(()),
        -2 => Err(AppError::Command(
            "Microphone permission was denied. Enable it in iOS Settings and retry.".into(),
        )),
        -6 => Err(AppError::Command(
            "iOS could not activate a record-capable audio session.".into(),
        )),
        code => Err(AppError::Command(format!(
            "Native iOS microphone capture could not start ({code})"
        ))),
    }
}

pub fn stop_microphone() {
    unsafe { crate::moonlight::native::nl_microphone_stop() };
}

pub fn set_microphone_muted(muted: bool) {
    unsafe { crate::moonlight::native::nl_microphone_set_muted(muted) };
}

pub fn set_microphone_bitrate(bitrate_bps: u32) -> AppResult<()> {
    let result = unsafe { crate::moonlight::native::nl_microphone_set_bitrate(bitrate_bps) };
    if result == 0 {
        Ok(())
    } else {
        Err(AppError::Command(format!(
            "Could not update the iOS Opus microphone bitrate ({result})"
        )))
    }
}

pub fn microphone_status() -> MicrophoneStatus {
    let mut native = crate::moonlight::native::nl_microphone_statistics_t::default();
    unsafe { crate::moonlight::native::nl_microphone_get_statistics(&mut native) };
    MicrophoneStatus {
        captured_samples: native.captured_samples,
        encoded_packets: native.encoded_packets,
        sent_bytes: native.sent_bytes,
        dropped_samples: native.dropped_samples,
        rtcp_reports: native.rtcp_reports,
        network_errors: native.network_errors,
        queue_depth_samples: native.queue_depth_samples,
        running: native.running != 0,
        suspended: native.suspended != 0,
        muted: native.muted != 0,
    }
}

import Foundation
import WireGuardKit

enum NolandWireGuardConfigError: LocalizedError {
    case invalidLine(String), missingInterface, missingValue(String), invalidValue(String), duplicateValue(String)
    var errorDescription: String? {
        switch self {
        case .invalidLine(let line): return "Invalid WireGuard line: \(line)"
        case .missingInterface: return "The WireGuard profile has no Interface section."
        case .missingValue(let key): return "The WireGuard profile is missing \(key)."
        case .invalidValue(let key): return "The WireGuard profile contains an invalid \(key)."
        case .duplicateValue(let key): return "The WireGuard profile repeats \(key)."
        }
    }
}

enum NolandWireGuardConfig {
    private enum Section { case none, interface, peer }

    static func parse(_ text: String, name: String) throws -> TunnelConfiguration {
        var section: Section = .none
        var interfaceValues: [String: [String]] = [:]
        var peerValues: [[String: [String]]] = []
        var currentPeer: [String: [String]] = [:]
        func append(_ key: String, _ value: String, to values: inout [String: [String]]) throws {
            let repeatable = Set(["address", "dns", "allowedips"])
            if values[key] != nil && !repeatable.contains(key) { throw NolandWireGuardConfigError.duplicateValue(key) }
            values[key, default: []].append(contentsOf: value.split(separator: ",").map {
                $0.trimmingCharacters(in: .whitespacesAndNewlines)
            }.filter { !$0.isEmpty })
        }
        for rawLine in text.split(whereSeparator: { $0.isNewline }) {
            let line = rawLine.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false)[0]
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if line.isEmpty { continue }
            if line.caseInsensitiveCompare("[Interface]") == .orderedSame {
                if case .peer = section { peerValues.append(currentPeer); currentPeer = [:] }
                section = .interface; continue
            }
            if line.caseInsensitiveCompare("[Peer]") == .orderedSame {
                if case .peer = section { peerValues.append(currentPeer); currentPeer = [:] }
                section = .peer; continue
            }
            guard let separator = line.firstIndex(of: "=") else { throw NolandWireGuardConfigError.invalidLine(line) }
            let key = line[..<separator].trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let value = line[line.index(after: separator)...].trimmingCharacters(in: .whitespacesAndNewlines)
            switch section {
            case .interface: try append(key, value, to: &interfaceValues)
            case .peer: try append(key, value, to: &currentPeer)
            case .none: throw NolandWireGuardConfigError.invalidLine(line)
            }
        }
        if case .peer = section { peerValues.append(currentPeer) }
        guard !interfaceValues.isEmpty else { throw NolandWireGuardConfigError.missingInterface }
        guard let privateText = interfaceValues["privatekey"]?.first else { throw NolandWireGuardConfigError.missingValue("Interface.PrivateKey") }
        guard let privateKey = PrivateKey(base64Key: privateText) else { throw NolandWireGuardConfigError.invalidValue("Interface.PrivateKey") }
        var interface = InterfaceConfiguration(privateKey: privateKey)
        interface.addresses = try (interfaceValues["address"] ?? []).map {
            guard let value = IPAddressRange(from: $0) else { throw NolandWireGuardConfigError.invalidValue("Interface.Address") }
            return value
        }
        if let value = interfaceValues["mtu"]?.first {
            guard let mtu = UInt16(value) else { throw NolandWireGuardConfigError.invalidValue("Interface.MTU") }
            interface.mtu = mtu
        }
        if let value = interfaceValues["listenport"]?.first {
            guard let port = UInt16(value) else { throw NolandWireGuardConfigError.invalidValue("Interface.ListenPort") }
            interface.listenPort = port
        }
        for value in interfaceValues["dns"] ?? [] {
            if let dns = DNSServer(from: value) { interface.dns.append(dns) } else { interface.dnsSearch.append(value) }
        }
        let peers = try peerValues.map { values -> PeerConfiguration in
            guard let text = values["publickey"]?.first else { throw NolandWireGuardConfigError.missingValue("Peer.PublicKey") }
            guard let key = PublicKey(base64Key: text) else { throw NolandWireGuardConfigError.invalidValue("Peer.PublicKey") }
            var peer = PeerConfiguration(publicKey: key)
            peer.allowedIPs = try (values["allowedips"] ?? []).map {
                guard let value = IPAddressRange(from: $0) else { throw NolandWireGuardConfigError.invalidValue("Peer.AllowedIPs") }
                return value
            }
            if let value = values["endpoint"]?.first {
                guard let endpoint = Endpoint(from: value) else { throw NolandWireGuardConfigError.invalidValue("Peer.Endpoint") }
                peer.endpoint = endpoint
            }
            if let value = values["persistentkeepalive"]?.first {
                guard let keepalive = UInt16(value) else { throw NolandWireGuardConfigError.invalidValue("Peer.PersistentKeepalive") }
                peer.persistentKeepAlive = keepalive
            }
            if let value = values["presharedkey"]?.first {
                guard let key = PreSharedKey(base64Key: value) else { throw NolandWireGuardConfigError.invalidValue("Peer.PresharedKey") }
                peer.preSharedKey = key
            }
            return peer
        }
        return TunnelConfiguration(name: name, interface: interface, peers: peers)
    }
}

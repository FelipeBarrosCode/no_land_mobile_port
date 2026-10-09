import CryptoKit
import Darwin
import Foundation
import NetworkExtension
import WireGuardKit

private struct UpdateRequest: Decodable {
    let configurationReference: String
    let configuration: String
    let expectedConfigFingerprint: String
    let configFingerprint: String
    let transitionID: UUID

    private enum CodingKeys: String, CodingKey {
        case configurationReference
        case configuration
        case expectedConfigFingerprint
        case configFingerprint
        case transitionID = "transitionId"
    }
}

final class PacketTunnelProvider: NEPacketTunnelProvider {
    private var adapter: WireGuardAdapter?
    private var configurationReference = ""
    private var configurationText = ""
    private var configFingerprint = ""
    private var launchID = ""

    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        guard let tunnel = protocolConfiguration as? NETunnelProviderProtocol,
              let provider = tunnel.providerConfiguration,
              let reference = provider["configurationReference"] as? String else {
            completionHandler(failure("The managed tunnel profile is invalid.")); return
        }
        do {
            guard let text = try NolandKeychain.get(account: reference) else {
                throw failure("The protected WireGuard configuration is missing.")
            }
            let configuration = try NolandWireGuardConfig.parse(text, name: "Noland Connect")
            let computed = fingerprint(text)
            // The Keychain value is the durable source of truth after live MTU
            // and endpoint transactions. The profile's initial fingerprint is
            // stale after those updates; only an explicit start request pins it.
            if let expected = options?["configFingerprint"] as? String, !expected.isEmpty, expected != computed {
                throw failure("The protected WireGuard configuration fingerprint is stale.")
            }
            configurationReference = reference
            configurationText = text
            configFingerprint = computed
            launchID = options?["launchID"] as? String ?? UUID().uuidString
            let adapter = WireGuardAdapter(with: self) { level, message in
                if level == .error { NSLog("Noland packet tunnel: %@", message) }
            }
            self.adapter = adapter
            adapter.start(tunnelConfiguration: configuration) { error in
                if let error { self.adapter = nil; completionHandler(error) }
                else { completionHandler(nil) }
            }
        } catch { completionHandler(error) }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        guard let adapter else { completionHandler(); return }
        adapter.stop { _ in self.adapter = nil; completionHandler() }
    }

    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        guard let completionHandler else { return }
        do {
            let envelope = try JSONDecoder().decode([String: String].self, from: messageData)
            switch envelope["method"] {
            case "runtime": runtime(completionHandler)
            case "update":
                guard let encoded = envelope["request"] else { throw failure("The update request is missing.") }
                try update(JSONDecoder().decode(UpdateRequest.self, from: Data(encoded.utf8)), completionHandler)
            default: throw failure("The packet tunnel control method is unknown.")
            }
        } catch { completionHandler(errorResponse(error)) }
    }

    private func update(_ request: UpdateRequest, _ reply: @escaping (Data?) -> Void) throws {
        guard let adapter else { throw failure("The WireGuard adapter is not active.") }
        guard request.configurationReference == configurationReference else { throw failure("The update targets another tunnel identity.") }
        guard request.expectedConfigFingerprint == configFingerprint else { throw failure("The tunnel changed before this transition could be applied.") }
        guard fingerprint(request.configuration) == request.configFingerprint else { throw failure("The updated configuration fingerprint is invalid.") }
        let next = try NolandWireGuardConfig.parse(request.configuration, name: "Noland Connect")
        let previousText = configurationText
        let previous = try NolandWireGuardConfig.parse(previousText, name: "Noland Connect")
        adapter.update(tunnelConfiguration: next) { updateError in
            if let updateError { reply(self.errorResponse(updateError)); return }
            do {
                try NolandKeychain.set(request.configuration, account: self.configurationReference)
                self.configurationText = request.configuration
                self.configFingerprint = request.configFingerprint
                self.runtime(reply, transitionID: request.transitionID)
            } catch {
                // A running configuration that was not durably committed is
                // forbidden. Restore it before reporting the failed mutation.
                adapter.update(tunnelConfiguration: previous) { rollbackError in
                    if let rollbackError {
                        reply(self.errorResponse(self.failure("Protected configuration commit failed (\(error.localizedDescription)); runtime rollback also failed (\(rollbackError.localizedDescription)).")))
                    } else {
                        reply(self.errorResponse(error))
                    }
                }
            }
        }
    }

    private func runtime(_ reply: @escaping (Data?) -> Void, transitionID: UUID? = nil) {
        guard let adapter else { reply(errorResponse(failure("The WireGuard adapter is not active."))); return }
        adapter.getRuntimeConfiguration { runtime in
            guard let runtime else { reply(self.errorResponse(self.failure("WireGuard runtime read-back failed."))); return }
            var rx: UInt64 = 0, tx: UInt64 = 0, lastHandshake: UInt64?
            var endpoint: String?, peerKey: String?
            for line in runtime.split(whereSeparator: { $0.isNewline }) {
                let pair = line.split(separator: "=", maxSplits: 1).map(String.init)
                guard pair.count == 2 else { continue }
                if pair[0] == "rx_bytes" { rx &+= UInt64(pair[1]) ?? 0 }
                if pair[0] == "tx_bytes" { tx &+= UInt64(pair[1]) ?? 0 }
                if pair[0] == "endpoint" { endpoint = pair[1] }
                if pair[0] == "public_key" { peerKey = PublicKey(hexKey: pair[1])?.base64Key }
                if pair[0] == "last_handshake_time_sec", let value = UInt64(pair[1]), value > 0 {
                    lastHandshake = max(lastHandshake ?? 0, value)
                }
            }
            let now = UInt64(Date().timeIntervalSince1970)
            guard let endpoint, let peerKey, let mtu = self.interfaceMtu(adapter.interfaceName) else {
                reply(self.errorResponse(self.failure("WireGuard endpoint, peer, or interface MTU read-back is unavailable.")))
                return
            }
            var response: [String: Any] = [
                "ok": true, "active": true, "launchId": self.launchID,
                "configFingerprint": self.configFingerprint, "peerPublicKey": peerKey,
                "endpoint": endpoint, "mtu": mtu, "rxBytes": rx, "txBytes": tx
            ]
            response["latestHandshakeAgeSecs"] = lastHandshake.map { now >= $0 ? now - $0 : 0 } ?? NSNull()
            if let transitionID { response["transitionId"] = transitionID.uuidString }
            reply(try? JSONSerialization.data(withJSONObject: response))
        }
    }

    private func interfaceMtu(_ name: String?) -> UInt32? {
        guard let name else { return nil }
        var first: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&first) == 0 else { return nil }
        defer { freeifaddrs(first) }
        var cursor = first
        while let current = cursor {
            let entry = current.pointee
            if let address = entry.ifa_addr, let data = entry.ifa_data,
               Int32(address.pointee.sa_family) == AF_LINK,
               String(cString: entry.ifa_name) == name {
                return data.assumingMemoryBound(to: if_data.self).pointee.ifi_mtu
            }
            cursor = entry.ifa_next
        }
        return nil
    }
    private func fingerprint(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }
    private func failure(_ message: String) -> NSError {
        NSError(domain: "NolandPacketTunnel", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }
    private func errorResponse(_ error: Error) -> Data? {
        try? JSONSerialization.data(withJSONObject: ["ok": false, "error": error.localizedDescription])
    }
}

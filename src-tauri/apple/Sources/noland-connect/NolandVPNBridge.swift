import CryptoKit
import Foundation
import NetworkExtension

private let packetTunnelBundleIdentifier = "noland.main.app.PacketTunnel"
private let vpnTimeout: TimeInterval = 20

private struct InstallRequest: Decodable {
    let configurationReference: String
    let configuration: String
    let operationID: String
    let instanceID: UInt64
    let launchID: String
    let configFingerprint: String

    private enum CodingKeys: String, CodingKey {
        case configurationReference
        case configuration
        case operationID = "operationId"
        case instanceID = "instanceId"
        case launchID = "launchId"
        case configFingerprint
    }
}
private struct ReferenceRequest: Codable { let configurationReference: String }
private struct UpdateRequest: Codable {
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
private struct VPNResponse: Encodable {
    let ok: Bool
    let status: String
    let configurationReference: String?
    let error: String?
}
private struct ConfigurationResponse: Encodable {
    let ok: Bool
    let configuration: String?
    let error: String?
}

private func fingerprint(_ configuration: String) -> String {
    SHA256.hash(data: Data(configuration.utf8)).map { String(format: "%02x", $0) }.joined()
}

private final class NolandVPNController {
    static let shared = NolandVPNController()
    private init() {}

    private func wait(_ operation: (@escaping (Error?) -> Void) -> Void) throws {
        let semaphore = DispatchSemaphore(value: 0)
        var callbackError: Error?
        operation { error in callbackError = error; semaphore.signal() }
        guard semaphore.wait(timeout: .now() + vpnTimeout) == .success else {
            throw NSError(domain: "NolandVPN", code: 1, userInfo: [NSLocalizedDescriptionKey: "The system VPN preferences request timed out."])
        }
        if let callbackError { throw callbackError }
    }

    private func managers() throws -> [NETunnelProviderManager] {
        let semaphore = DispatchSemaphore(value: 0)
        var loaded: [NETunnelProviderManager] = []
        var callbackError: Error?
        NETunnelProviderManager.loadAllFromPreferences { managers, error in
            loaded = managers ?? []; callbackError = error; semaphore.signal()
        }
        guard semaphore.wait(timeout: .now() + vpnTimeout) == .success else {
            throw NSError(domain: "NolandVPN", code: 2, userInfo: [NSLocalizedDescriptionKey: "Loading the VPN configuration timed out."])
        }
        if let callbackError { throw callbackError }
        return loaded
    }

    private func matching(_ reference: String) throws -> [NETunnelProviderManager] {
        try managers().filter {
            guard let tunnel = $0.protocolConfiguration as? NETunnelProviderProtocol else { return false }
            return tunnel.providerBundleIdentifier == packetTunnelBundleIdentifier
                && tunnel.providerConfiguration?["configurationReference"] as? String == reference
        }
    }

    private func manager(_ reference: String) throws -> NETunnelProviderManager? {
        let matches = try matching(reference)
        guard matches.count <= 1 else {
            throw NSError(domain: "NolandVPN", code: 3, userInfo: [NSLocalizedDescriptionKey: "Multiple managed VPN profiles claim this tunnel identity. Remove the duplicate profiles before reconnecting."])
        }
        return matches.first
    }

    private func active(_ manager: NETunnelProviderManager) -> Bool {
        ![.invalid, .disconnected].contains(manager.connection.status)
    }

    private func stopAndWait(_ manager: NETunnelProviderManager) {
        guard active(manager) else { return }
        manager.connection.stopVPNTunnel()
        let deadline = Date().addingTimeInterval(8)
        while active(manager), Date() < deadline { Thread.sleep(forTimeInterval: 0.1) }
    }

    private func waitForConnection(_ manager: NETunnelProviderManager) throws {
        let deadline = Date().addingTimeInterval(vpnTimeout)
        while Date() < deadline {
            switch manager.connection.status {
            case .connected: return
            case .invalid, .disconnected:
                throw NSError(domain: "NolandVPN", code: 4, userInfo: [NSLocalizedDescriptionKey: "The packet tunnel stopped before becoming connected."])
            default: Thread.sleep(forTimeInterval: 0.1)
            }
        }
        throw NSError(domain: "NolandVPN", code: 5, userInfo: [NSLocalizedDescriptionKey: "The packet tunnel did not connect before the deadline."])
    }

    func install(_ request: InstallRequest) throws -> VPNResponse {
        _ = try NolandWireGuardConfig.parse(request.configuration, name: "Noland \(request.instanceID)")
        guard fingerprint(request.configuration) == request.configFingerprint else {
            throw NSError(domain: "NolandVPN", code: 6, userInfo: [NSLocalizedDescriptionKey: "The WireGuard configuration fingerprint does not match its contents."])
        }
        let previousConfiguration = try NolandKeychain.get(account: request.configurationReference)
        let existing = try manager(request.configurationReference)
        let manager = existing ?? NETunnelProviderManager()
        let created = existing == nil
        let previousProtocol = manager.protocolConfiguration?.copy() as? NEVPNProtocol
        let previousDescription = manager.localizedDescription
        let previousEnabled = manager.isEnabled
        let otherManagers = try managers().filter {
            guard $0 !== manager, let tunnel = $0.protocolConfiguration as? NETunnelProviderProtocol else { return false }
            return tunnel.providerBundleIdentifier == packetTunnelBundleIdentifier
        }
        let previouslyActiveOthers = otherManagers.filter(active)
        try NolandKeychain.set(request.configuration, account: request.configurationReference)
        do {
            stopAndWait(manager)
            let tunnel = NETunnelProviderProtocol()
            tunnel.providerBundleIdentifier = packetTunnelBundleIdentifier
            tunnel.serverAddress = "Noland instance \(request.instanceID)"
            tunnel.disconnectOnSleep = false
            tunnel.providerConfiguration = [
                "schemaVersion": 2,
                "configurationReference": request.configurationReference,
                "configFingerprint": request.configFingerprint
            ]
            manager.protocolConfiguration = tunnel
            manager.localizedDescription = "Noland Connect"
            manager.isEnabled = true
            manager.isOnDemandEnabled = false
            try wait { manager.saveToPreferences(completionHandler: $0) }
            try wait { manager.loadFromPreferences(completionHandler: $0) }
            // Exactly one Noland tunnel may own the 10.77.0.0 route.
            for other in otherManagers { stopAndWait(other) }
            try manager.connection.startVPNTunnel(options: [
                "operationID": request.operationID as NSString,
                "configurationReference": request.configurationReference as NSString,
                "launchID": request.launchID as NSString,
                "configFingerprint": request.configFingerprint as NSString
            ])
            try waitForConnection(manager)
            return VPNResponse(ok: true, status: "connected", configurationReference: request.configurationReference, error: nil)
        } catch {
            if let previousConfiguration { try? NolandKeychain.set(previousConfiguration, account: request.configurationReference) }
            else { try? NolandKeychain.delete(account: request.configurationReference) }
            if created { try? wait { manager.removeFromPreferences(completionHandler: $0) } }
            else {
                manager.protocolConfiguration = previousProtocol
                manager.localizedDescription = previousDescription
                manager.isEnabled = previousEnabled
                try? wait { manager.saveToPreferences(completionHandler: $0) }
            }
            for other in previouslyActiveOthers { try? other.connection.startVPNTunnel() }
            throw error
        }
    }

    func reconnect(_ reference: String) throws -> VPNResponse {
        guard let manager = try manager(reference), try NolandKeychain.get(account: reference) != nil else {
            return VPNResponse(ok: false, status: "not_configured", configurationReference: reference, error: "The requested protected tunnel is not configured.")
        }
        stopAndWait(manager)
        try manager.connection.startVPNTunnel(options: ["configurationReference": reference as NSString])
        try waitForConnection(manager)
        return VPNResponse(ok: true, status: "connected", configurationReference: reference, error: nil)
    }

    func stop(_ reference: String) throws -> VPNResponse {
        guard let manager = try manager(reference) else {
            return VPNResponse(ok: true, status: "not_configured", configurationReference: reference, error: nil)
        }
        stopAndWait(manager)
        return VPNResponse(ok: true, status: statusName(manager.connection.status), configurationReference: reference, error: nil)
    }

    func remove(_ reference: String) throws -> VPNResponse {
        for manager in try matching(reference) {
            stopAndWait(manager)
            try wait { manager.removeFromPreferences(completionHandler: $0) }
        }
        try NolandKeychain.delete(account: reference)
        return VPNResponse(ok: true, status: "not_configured", configurationReference: nil, error: nil)
    }

    func status(_ reference: String) throws -> VPNResponse {
        let matches = try matching(reference)
        if matches.count > 1 {
            return VPNResponse(ok: false, status: "recovery_required", configurationReference: reference, error: "Multiple managed profiles claim this tunnel identity.")
        }
        guard let manager = matches.first else {
            return VPNResponse(ok: true, status: try NolandKeychain.get(account: reference) == nil ? "not_configured" : "stored_inactive", configurationReference: reference, error: nil)
        }
        guard try NolandKeychain.get(account: reference) != nil else {
            return VPNResponse(ok: false, status: "recovery_required", configurationReference: reference, error: "The protected configuration is missing from the shared Keychain.")
        }
        return VPNResponse(ok: true, status: statusName(manager.connection.status), configurationReference: reference, error: nil)
    }

    func providerMessage(_ reference: String, payload: Data) throws -> Data {
        guard let manager = try manager(reference), let session = manager.connection as? NETunnelProviderSession,
              manager.connection.status == .connected || manager.connection.status == .reasserting else {
            throw NSError(domain: "NolandVPN", code: 7, userInfo: [NSLocalizedDescriptionKey: "The managed packet tunnel is not connected."])
        }
        let semaphore = DispatchSemaphore(value: 0)
        var result: Data?
        try session.sendProviderMessage(payload) { response in result = response; semaphore.signal() }
        guard semaphore.wait(timeout: .now() + vpnTimeout) == .success, let result else {
            throw NSError(domain: "NolandVPN", code: 8, userInfo: [NSLocalizedDescriptionKey: "The packet tunnel did not answer the control request."])
        }
        return result
    }

    private func statusName(_ status: NEVPNStatus) -> String {
        switch status {
        case .invalid: return "invalid"
        case .disconnected: return "disconnected"
        case .connecting: return "connecting"
        case .connected: return "connected"
        case .reasserting: return "reasserting"
        case .disconnecting: return "disconnecting"
        @unknown default: return "unknown"
        }
    }
}

private func decode<T: Decodable>(_ pointer: UnsafePointer<CChar>?, as: T.Type) throws -> T {
    guard let pointer else { throw NSError(domain: "NolandVPN", code: 9, userInfo: [NSLocalizedDescriptionKey: "Missing VPN request."]) }
    return try JSONDecoder().decode(T.self, from: Data(String(cString: pointer).utf8))
}
private func encodedPointer<T: Encodable>(_ value: T) -> UnsafeMutablePointer<CChar>? {
    let data = (try? JSONEncoder().encode(value)) ?? Data("{\"ok\":false,\"status\":\"failed\",\"error\":\"encoding failed\"}".utf8)
    return strdup(String(decoding: data, as: UTF8.self))
}
private func statusBridge(_ operation: () throws -> VPNResponse) -> UnsafeMutablePointer<CChar>? {
    do { return encodedPointer(try operation()) }
    catch { return encodedPointer(VPNResponse(ok: false, status: "failed", configurationReference: nil, error: error.localizedDescription)) }
}
private func dataBridge(_ operation: () throws -> Data) -> UnsafeMutablePointer<CChar>? {
    do { return strdup(String(decoding: try operation(), as: UTF8.self)) }
    catch {
        let data = (try? JSONSerialization.data(withJSONObject: ["ok": false, "error": error.localizedDescription]))
            ?? Data("{\"ok\":false,\"error\":\"encoding failed\"}".utf8)
        return strdup(String(decoding: data, as: UTF8.self))
    }
}

@_cdecl("nl_apple_vpn_install_and_start")
public func nlAppleVPNInstallAndStart(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    statusBridge { try NolandVPNController.shared.install(decode(pointer, as: InstallRequest.self)) }
}
@_cdecl("nl_apple_vpn_reconnect")
public func nlAppleVPNReconnect(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    statusBridge { try NolandVPNController.shared.reconnect(decode(pointer, as: ReferenceRequest.self).configurationReference) }
}
@_cdecl("nl_apple_vpn_stop")
public func nlAppleVPNStop(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    statusBridge { try NolandVPNController.shared.stop(decode(pointer, as: ReferenceRequest.self).configurationReference) }
}
@_cdecl("nl_apple_vpn_remove")
public func nlAppleVPNRemove(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    statusBridge { try NolandVPNController.shared.remove(decode(pointer, as: ReferenceRequest.self).configurationReference) }
}
@_cdecl("nl_apple_vpn_status")
public func nlAppleVPNStatus(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    statusBridge { try NolandVPNController.shared.status(decode(pointer, as: ReferenceRequest.self).configurationReference) }
}
@_cdecl("nl_apple_vpn_update")
public func nlAppleVPNUpdate(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    dataBridge {
        let request = try decode(pointer, as: UpdateRequest.self)
        return try NolandVPNController.shared.providerMessage(request.configurationReference, payload: JSONEncoder().encode(["method": "update", "request": String(data: try JSONEncoder().encode(request), encoding: .utf8)!]))
    }
}
@_cdecl("nl_apple_vpn_runtime")
public func nlAppleVPNRuntime(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    dataBridge {
        let request = try decode(pointer, as: ReferenceRequest.self)
        return try NolandVPNController.shared.providerMessage(request.configurationReference, payload: JSONEncoder().encode(["method": "runtime"]))
    }
}
@_cdecl("nl_apple_vpn_configuration")
public func nlAppleVPNConfiguration(_ pointer: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    do {
        let request = try decode(pointer, as: ReferenceRequest.self)
        guard let configuration = try NolandKeychain.get(account: request.configurationReference) else {
            throw NSError(domain: "NolandVPN", code: 10, userInfo: [NSLocalizedDescriptionKey: "The protected WireGuard configuration is missing."])
        }
        return encodedPointer(ConfigurationResponse(ok: true, configuration: configuration, error: nil))
    } catch {
        return encodedPointer(ConfigurationResponse(ok: false, configuration: nil, error: error.localizedDescription))
    }
}
@_cdecl("nl_apple_string_free")
public func nlAppleStringFree(_ pointer: UnsafeMutablePointer<CChar>?) { free(pointer) }

import Foundation
import Security

enum NolandKeychainError: LocalizedError {
    case missingAccessGroup
    case invalidUTF8
    case status(OSStatus)

    var errorDescription: String? {
        switch self {
        case .missingAccessGroup: return "The shared Keychain access group is not configured."
        case .invalidUTF8: return "The protected value is not valid UTF-8."
        case .status(let status):
            return SecCopyErrorMessageString(status, nil) as String? ?? "Keychain error \(status)."
        }
    }
}

enum NolandKeychain {
    private static let service = "com.noland.connect.mobile.tunnel"

    private static var accessGroup: String {
        get throws {
            guard let group = Bundle.main.object(forInfoDictionaryKey: "NolandKeychainAccessGroup") as? String,
                  !group.isEmpty, !group.contains("$(") else { throw NolandKeychainError.missingAccessGroup }
            return group
        }
    }

    static func set(_ value: String, account: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: try accessGroup
        ]
        let attributes: [String: Any] = [
            kSecValueData as String: Data(value.utf8),
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        ]
        let updated = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if updated == errSecSuccess { return }
        guard updated == errSecItemNotFound else { throw NolandKeychainError.status(updated) }
        var inserted = query
        attributes.forEach { inserted[$0.key] = $0.value }
        let status = SecItemAdd(inserted as CFDictionary, nil)
        guard status == errSecSuccess else { throw NolandKeychainError.status(status) }
    }

    static func get(account: String) throws -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: try accessGroup,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecReturnData as String: true
        ]
        var value: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &value)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = value as? Data else { throw NolandKeychainError.status(status) }
        guard let text = String(data: data, encoding: .utf8) else { throw NolandKeychainError.invalidUTF8 }
        return text
    }

    static func delete(account: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: try accessGroup
        ]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw NolandKeychainError.status(status) }
    }
}

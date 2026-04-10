import Foundation
import Security

enum KeychainService {
    private static let servicePrefix = "com.claude-dashboard"
    private static var cache: [String: String] = [:]

    static func save(key: String, value: String) {
        cache[key] = value

        let data = value.data(using: .utf8)!
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
        ]

        SecItemDelete(query as CFDictionary)

        var addQuery = query
        addQuery[kSecValueData as String] = data
        addQuery[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock

        SecItemAdd(addQuery as CFDictionary, nil)
    }

    static func load(key: String) -> String? {
        if let cached = cache[key] {
            return cached
        }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess, let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else {
            return nil
        }

        cache[key] = value
        return value
    }

    static func delete(key: String) {
        cache.removeValue(forKey: key)

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
        ]

        SecItemDelete(query as CFDictionary)
    }

    // Keys for account session storage
    static func sessionKey(for accountId: UUID) -> String {
        "\(accountId.uuidString)-sessionKey"
    }
}

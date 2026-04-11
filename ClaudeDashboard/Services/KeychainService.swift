import Foundation
import Security

actor KeychainService {
    static let shared = KeychainService()

    private let servicePrefix = "com.claude-dashboard"
    private var cache: [String: String] = [:]

    func save(key: String, value: String) {
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

    func load(key: String) -> String? {
        if let cached = cache[key] {
            return cached
        }

        // Bulk-load all items for our service on first miss — single Keychain prompt
        if !didBulkLoad {
            bulkLoad()
            if let cached = cache[key] {
                return cached
            }
        }

        // Fallback: individual lookup (shouldn't normally be needed)
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

    private var didBulkLoad = false

    private func bulkLoad() {
        didBulkLoad = true

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecReturnData as String: true,
            kSecReturnAttributes as String: true,
            kSecMatchLimit as String: kSecMatchLimitAll,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess, let items = result as? [[String: Any]] else { return }

        for item in items {
            guard let account = item[kSecAttrAccount as String] as? String,
                  let data = item[kSecValueData as String] as? Data,
                  let value = String(data: data, encoding: .utf8) else { continue }
            cache[account] = value
        }
    }

    func delete(key: String) {
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

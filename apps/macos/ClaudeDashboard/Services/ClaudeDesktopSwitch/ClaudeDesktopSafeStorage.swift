import Foundation
import Security

/// Electron `safeStorage` as Claude Desktop uses it for `config.json` blobs:
/// base64 of `v10` + AES-128-CBC ciphertext, keyed from the "Claude Safe Storage"
/// Keychain password. Unlike cookie values, the plaintext has no domain-hash prefix.
///
///     let blob = ClaudeDesktopSafeStorage.encrypt(json, password: pw)
///     let json = ClaudeDesktopSafeStorage.decrypt(blob!, password: pw)
enum ClaudeDesktopSafeStorage {
    static let keychainService = "Claude Safe Storage"
    private static let prefix = Data("v10".utf8)
    private static let iv = Data(repeating: 0x20, count: 16)

    /// The Safe Storage password, read through `SecItemCopyMatching` (no prompt path).
    static func password() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func decrypt(_ base64Blob: String, password: String) -> Data? {
        guard let blob = Data(base64Encoded: base64Blob),
              blob.count > prefix.count,
              blob.prefix(prefix.count) == prefix else { return nil }
        return BrowserCookieService.aes128CBC(
            Data(blob.dropFirst(prefix.count)),
            key: BrowserCookieService.deriveKey(from: password),
            iv: iv,
            encrypt: false
        )
    }

    static func encrypt(_ plaintext: Data, password: String) -> String? {
        guard let cipher = BrowserCookieService.aes128CBC(
            plaintext,
            key: BrowserCookieService.deriveKey(from: password),
            iv: iv,
            encrypt: true
        ) else { return nil }
        return (prefix + cipher).base64EncodedString()
    }
}

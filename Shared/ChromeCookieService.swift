import Foundation
import CommonCrypto
import SQLite3

struct ChromeProfile {
    let path: String        // e.g. "Profile 1"
    let displayName: String // e.g. "Harry"
    let googleEmail: String // e.g. "hai@gotitapp.co" — from user_name field
}

struct ChromeCookieResult {
    let sessionKey: String?
    let orgId: String?
}

enum ChromeCookieService {

    private static let chromeBasePath = NSHomeDirectory()
        + "/Library/Application Support/Google/Chrome"

    /// Cached encryption key — avoids repeated Keychain prompts per app session
    private static var cachedEncryptionKey: Data?

    // MARK: - Profile Scanning

    static func scanProfiles() -> [ChromeProfile] {
        let localStatePath = chromeBasePath + "/Local State"
        guard let data = FileManager.default.contents(atPath: localStatePath) else {
            return []
        }
        return parseProfiles(from: data)
    }

    static func parseProfiles(from data: Data) -> [ChromeProfile] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let profile = json["profile"] as? [String: Any],
              let infoCache = profile["info_cache"] as? [String: Any] else {
            return []
        }

        // Only include profiles that currently have open windows
        let activeProfiles = Set(profile["last_active_profiles"] as? [String] ?? [])

        return infoCache.compactMap { (key, value) in
            guard activeProfiles.contains(key),
                  let info = value as? [String: Any],
                  let name = info["name"] as? String else {
                return nil
            }
            let googleEmail = info["user_name"] as? String ?? ""
            return ChromeProfile(path: key, displayName: name, googleEmail: googleEmail)
        }
        .sorted { $0.path < $1.path }
    }

    // MARK: - Cookie Extraction

    static func extractCookies(for profilePath: String) -> ChromeCookieResult {
        guard let encryptionKey = getChromeEncryptionKey() else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        return extractCookies(for: profilePath, encryptionKey: encryptionKey)
    }

    static func extractCookies(for profilePath: String, encryptionKey: Data) -> ChromeCookieResult {

        let dbPath = chromeBasePath + "/\(profilePath)/Cookies"

        // Copy DB to temp location — Chrome holds WAL lock while running,
        // preventing direct read-only access
        let tempDir = NSTemporaryDirectory()
        let tempPath = tempDir + "claude-dashboard-cookies-\(profilePath.replacingOccurrences(of: " ", with: "_")).db"
        try? FileManager.default.removeItem(atPath: tempPath)
        guard (try? FileManager.default.copyItem(atPath: dbPath, toPath: tempPath)) != nil else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        // Also copy WAL and SHM if they exist
        for suffix in ["-wal", "-shm"] {
            let src = dbPath + suffix
            let dst = tempPath + suffix
            try? FileManager.default.removeItem(atPath: dst)
            try? FileManager.default.copyItem(atPath: src, toPath: dst)
        }
        defer {
            try? FileManager.default.removeItem(atPath: tempPath)
            try? FileManager.default.removeItem(atPath: tempPath + "-wal")
            try? FileManager.default.removeItem(atPath: tempPath + "-shm")
        }

        var db: OpaquePointer?
        guard sqlite3_open_v2(tempPath, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        defer { sqlite3_close(db) }

        var sessionKey: String?
        var orgId: String?

        let query = """
            SELECT name, encrypted_value FROM cookies
            WHERE (host_key = '.claude.ai' OR host_key = 'claude.ai')
            AND name IN ('sessionKey', 'lastActiveOrg')
        """

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, query, -1, &stmt, nil) == SQLITE_OK else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        defer { sqlite3_finalize(stmt) }

        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let namePtr = sqlite3_column_text(stmt, 0) else { continue }
            let name = String(cString: namePtr)

            let blobSize = sqlite3_column_bytes(stmt, 1)
            guard blobSize > 0,
                  let blobPtr = sqlite3_column_blob(stmt, 1) else { continue }
            let encrypted = Data(bytes: blobPtr, count: Int(blobSize))

            guard let decrypted = decryptCookieValue(encrypted, withKey: encryptionKey) else {
                continue
            }

            switch name {
            case "sessionKey":
                sessionKey = decrypted
            case "lastActiveOrg":
                orgId = decrypted
            default:
                break
            }
        }

        return ChromeCookieResult(sessionKey: sessionKey, orgId: orgId)
    }

    // MARK: - Profiles with Claude Sessions

    static func profilesWithClaudeSessions() -> [(profile: ChromeProfile, cookies: ChromeCookieResult)] {
        guard let encryptionKey = getChromeEncryptionKey() else { return [] }
        let profiles = scanProfiles()
        return profiles.compactMap { profile in
            let cookies = extractCookies(for: profile.path, encryptionKey: encryptionKey)
            guard cookies.sessionKey != nil else { return nil }
            return (profile: profile, cookies: cookies)
        }
    }

    // MARK: - Crypto

    static func getChromeEncryptionKey() -> Data? {
        if let cached = cachedEncryptionKey { return cached }
        guard let passphrase = getChromeSafeStoragePassword() else { return nil }
        let key = deriveKey(from: passphrase)
        cachedEncryptionKey = key
        return key
    }

    static func deriveKey(from passphrase: String) -> Data {
        let salt = "saltysalt".data(using: .utf8)!
        var derivedKey = Data(count: 16)

        _ = derivedKey.withUnsafeMutableBytes { derivedKeyBytes in
            salt.withUnsafeBytes { saltBytes in
                CCKeyDerivationPBKDF(
                    CCPBKDFAlgorithm(kCCPBKDF2),
                    passphrase,
                    passphrase.utf8.count,
                    saltBytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    salt.count,
                    CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA1),
                    1003,
                    derivedKeyBytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    16
                )
            }
        }

        return derivedKey
    }

    static func decryptCookieValue(_ encrypted: Data, withKey key: Data) -> String? {
        guard encrypted.count > 3,
              encrypted[0] == 0x76, encrypted[1] == 0x31, encrypted[2] == 0x30 else {
            return nil
        }

        let ciphertext = encrypted.dropFirst(3)
        let iv = Data(repeating: 0x20, count: 16)

        var decryptedData = Data(count: ciphertext.count + kCCBlockSizeAES128)
        let decryptedDataCapacity = decryptedData.count
        var decryptedLength = 0

        let status = decryptedData.withUnsafeMutableBytes { decryptedBytes in
            ciphertext.withUnsafeBytes { ciphertextBytes in
                key.withUnsafeBytes { keyBytes in
                    iv.withUnsafeBytes { ivBytes in
                        CCCrypt(
                            CCOperation(kCCDecrypt),
                            CCAlgorithm(kCCAlgorithmAES128),
                            CCOptions(kCCOptionPKCS7Padding),
                            keyBytes.baseAddress, key.count,
                            ivBytes.baseAddress,
                            ciphertextBytes.baseAddress, ciphertext.count,
                            decryptedBytes.baseAddress, decryptedDataCapacity,
                            &decryptedLength
                        )
                    }
                }
            }
        }

        guard status == kCCSuccess else { return nil }

        decryptedData.count = decryptedLength

        // Try full data as UTF-8 first
        if let result = String(data: decryptedData, encoding: .utf8) {
            return result
        }

        // Chrome DB v24+ prepends 32-byte domain hash (non-UTF8 binary)
        // Strip it and try again
        if decryptedLength > 32 {
            let stripped = Data(decryptedData.dropFirst(32))
            if let result = String(data: stripped, encoding: .utf8) {
                return result
            }
        }

        return nil
    }

    private static func getChromeSafeStoragePassword() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "Chrome Safe Storage",
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess, let data = result as? Data else {
            return nil
        }

        return String(data: data, encoding: .utf8)
    }
}

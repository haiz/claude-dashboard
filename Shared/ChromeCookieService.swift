import Foundation
import CommonCrypto
import SQLite3

struct BrowserProfile {
    let path: String        // e.g. "Profile 1"
    let displayName: String // e.g. "Harry"
    let googleEmail: String // e.g. "hai@gotitapp.co" — from user_name field
    let browser: Browser
}

struct ChromeCookieResult {
    let sessionKey: String?
    let orgId: String?
}

enum BrowserCookieService {

    /// Cached encryption key theo từng browser — tránh prompt Keychain lặp lại.
    private static var cachedEncryptionKeys: [Browser: Data] = [:]

    // MARK: - Profile Scanning

    static func scanProfiles(browser: Browser) -> [BrowserProfile] {
        let localStatePath = browser.basePath + "/Local State"
        guard let data = FileManager.default.contents(atPath: localStatePath) else {
            return []
        }
        let parsed = parseProfiles(from: data, browser: browser)
        if !parsed.isEmpty { return parsed }

        // Fallback: vài browser (vd Arc) có thể không điền info_cache như Chrome.
        // Quét trực tiếp các thư mục con chứa file Cookies.
        let names = (try? FileManager.default.contentsOfDirectory(atPath: browser.basePath)) ?? []
        return profilesFromDirectoryNames(names, browser: browser).filter { profile in
            FileManager.default.fileExists(atPath: browser.basePath + "/\(profile.path)/Cookies")
        }
    }

    static func parseProfiles(from data: Data, browser: Browser) -> [BrowserProfile] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let profile = json["profile"] as? [String: Any],
              let infoCache = profile["info_cache"] as? [String: Any] else {
            return []
        }

        return infoCache.compactMap { (key, value) in
            guard let info = value as? [String: Any],
                  let name = info["name"] as? String else {
                return nil
            }
            let googleEmail = info["user_name"] as? String ?? ""
            return BrowserProfile(path: key, displayName: name, googleEmail: googleEmail, browser: browser)
        }
        .sorted { $0.path < $1.path }
    }

    /// Pure helper: lọc tên thư mục con theo pattern profile của Chromium ("Default", "Profile N").
    static func profilesFromDirectoryNames(_ names: [String], browser: Browser) -> [BrowserProfile] {
        names
            .filter { $0 == "Default" || $0.hasPrefix("Profile ") }
            .sorted()
            .map { BrowserProfile(path: $0, displayName: $0, googleEmail: "", browser: browser) }
    }

    // MARK: - Cookie Extraction

    static func extractCookies(for profilePath: String, browser: Browser) -> ChromeCookieResult {
        guard let encryptionKey = getEncryptionKey(browser: browser) else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        return extractCookies(for: profilePath, browser: browser, encryptionKey: encryptionKey)
    }

    static func extractCookies(for profilePath: String, browser: Browser, encryptionKey: Data) -> ChromeCookieResult {

        let dbPath = browser.basePath + "/\(profilePath)/Cookies"

        // Copy DB to temp location — Chrome holds WAL lock while running,
        // preventing direct read-only access
        let tempDir = NSTemporaryDirectory()
        let tempPath = tempDir + "claude-dashboard-cookies-\(browser.rawValue)-\(profilePath.replacingOccurrences(of: " ", with: "_")).db"
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

    static func profilesWithClaudeSessions(browser: Browser) -> [(profile: BrowserProfile, cookies: ChromeCookieResult)] {
        guard let encryptionKey = getEncryptionKey(browser: browser) else { return [] }
        let profiles = scanProfiles(browser: browser)
        return profiles.compactMap { profile in
            let cookies = extractCookies(for: profile.path, browser: browser, encryptionKey: encryptionKey)
            guard cookies.sessionKey != nil else { return nil }
            return (profile: profile, cookies: cookies)
        }
    }

    /// Các browser đã cài (có "Local State"). Chrome đứng đầu nếu có.
    static func installedBrowsers() -> [Browser] {
        Browser.allCases.filter {
            FileManager.default.fileExists(atPath: $0.basePath + "/Local State")
        }
    }

    // MARK: - Crypto

    static func getEncryptionKey(browser: Browser) -> Data? {
        if let cached = cachedEncryptionKeys[browser] { return cached }
        guard let passphrase = getSafeStoragePassword(browser: browser) else { return nil }
        let key = deriveKey(from: passphrase)
        cachedEncryptionKeys[browser] = key
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

    private static func getSafeStoragePassword(browser: Browser) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: browser.keychainService,
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

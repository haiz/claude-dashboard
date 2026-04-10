import Foundation
import CommonCrypto
import SQLite3

struct ChromeProfile {
    let path: String
    let displayName: String
}

struct ChromeCookieResult {
    let sessionKey: String?
    let orgId: String?
}

enum ChromeCookieService {

    private static let chromeBasePath = NSHomeDirectory()
        + "/Library/Application Support/Google/Chrome"

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

        return infoCache.compactMap { (key, value) in
            guard let info = value as? [String: Any],
                  let name = info["name"] as? String else {
                return nil
            }
            return ChromeProfile(path: key, displayName: name)
        }
        .sorted { $0.path < $1.path }
    }

    // MARK: - Cookie Extraction

    static func extractCookies(for profilePath: String) -> ChromeCookieResult {
        guard let encryptionKey = getChromeEncryptionKey() else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }

        let dbPath = chromeBasePath + "/\(profilePath)/Cookies"
        var db: OpaquePointer?

        guard sqlite3_open_v2(dbPath, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
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
        let profiles = scanProfiles()
        return profiles.compactMap { profile in
            let cookies = extractCookies(for: profile.path)
            guard cookies.sessionKey != nil else { return nil }
            return (profile: profile, cookies: cookies)
        }
    }

    // MARK: - Crypto

    static func getChromeEncryptionKey() -> Data? {
        let passphrase = getChromeSafeStoragePassword()
        guard let passphrase else { return nil }
        return deriveKey(from: passphrase)
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

        // Chrome DB v24+ may prepend 32-byte domain hash
        if decryptedLength > 32 {
            let asString = String(data: decryptedData, encoding: .utf8)
            if asString == nil || asString?.contains("\0") == true {
                let stripped = decryptedData.dropFirst(32)
                if let result = String(data: stripped, encoding: .utf8) {
                    return result
                }
            }
            if let result = asString {
                return result
            }
        }

        return String(data: decryptedData, encoding: .utf8)
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

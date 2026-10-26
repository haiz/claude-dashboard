import Foundation

enum ClaudeDesktopConfigError: Error, Equatable {
    /// `config.json` exists but is not a JSON object; nothing was written.
    case unreadableConfig
    /// The grant could not be encrypted with the Safe Storage password.
    case encryptionFailed
}

/// Claude Desktop's signed-in grant: `oauth:tokenCacheV2` and `lastKnownAccountUuid` in `config.json`.
protocol ClaudeDesktopConfigReadingWriting {
    /// The account Desktop is signed in as and its grant; nil when the file, the cache or
    /// the uuid is absent or cannot be decrypted. Never throws for absence.
    func readActiveGrant() throws -> (accountUuid: String, oauth: OAuthCredential)?
    /// Replaces the grant and `lastKnownAccountUuid`, drops the legacy `oauth:tokenCache`,
    /// and keeps every other top-level key. Throws `ClaudeDesktopConfigError.unreadableConfig`
    /// when an existing file is not a JSON object (nothing is written).
    func writeGrant(_ oauth: OAuthCredential, accountUuid: String, orgUuid: String) throws
}

/// `~/Library/Application Support/Claude/config.json`. The cache is one safeStorage blob
/// whose plaintext maps `"<clientId>:<orgId>:<audience>:<scopes>"` to the grant; Desktop
/// rewrites the keys itself on relaunch, so the key written here only has to resolve.
///
///     let cfg = ClaudeDesktopConfigFile(fileURL: ClaudeDesktopConfigFile.defaultURL, password: pw)
///     try cfg.writeGrant(oauth, accountUuid: uuid, orgUuid: org)
struct ClaudeDesktopConfigFile: ClaudeDesktopConfigReadingWriting {
    static let defaultURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude/config.json")
    static let scope = ClaudeCodeOAuthClient.scope

    private static let cacheKey = "oauth:tokenCacheV2"
    private static let legacyKey = "oauth:tokenCache"
    private static let uuidKey = "lastKnownAccountUuid"
    /// Grant fields shared by Desktop's entry and `OAuthCredential`, as (entry key, credential key).
    private static let fields = [("token", "accessToken"), ("refreshToken", "refreshToken"),
                                 ("expiresAt", "expiresAt"), ("subscriptionType", "subscriptionType"),
                                 ("rateLimitTier", "rateLimitTier")]

    private let fileURL: URL
    private let password: String

    /// Both explicit so tests never reach the real file or Keychain.
    init(fileURL: URL, password: String) {
        self.fileURL = fileURL
        self.password = password
    }

    func readActiveGrant() throws -> (accountUuid: String, oauth: OAuthCredential)? {
        guard let data = try? Data(contentsOf: fileURL),
              let root = OAuthAccountJSON.object(data),
              let uuid = root[Self.uuidKey] as? String,
              let blob = root[Self.cacheKey] as? String,
              let plain = ClaudeDesktopSafeStorage.decrypt(blob, password: password),
              let map = OAuthAccountJSON.object(plain),
              let entry = map.sorted(by: { $0.key < $1.key }).lazy.compactMap({ $0.value as? [String: Any] }).first
        else { return nil }
        var object: [String: Any] = [:]
        for (from, to) in Self.fields { object[to] = entry[from] }
        guard let oauth = OAuthCredential(object: object) else { return nil }
        return (uuid, oauth)
    }

    func writeGrant(_ oauth: OAuthCredential, accountUuid: String, orgUuid: String) throws {
        var root: [String: Any] = [:]
        if FileManager.default.fileExists(atPath: fileURL.path) {
            // Present but unreadable (e.g. permissions) must not be treated as absent.
            guard let data = try? Data(contentsOf: fileURL),
                  let existing = OAuthAccountJSON.object(data) else { throw ClaudeDesktopConfigError.unreadableConfig }
            root = existing
        }
        let source = oauth.object
        var entry: [String: Any] = [:]
        for (to, from) in Self.fields { entry[to] = source[from] }
        let key = "\(accountUuid):\(orgUuid):https://api.anthropic.com:\(Self.scope)"
        guard let plain = OAuthAccountJSON.canonical([key: entry]),
              let blob = ClaudeDesktopSafeStorage.encrypt(plain, password: password)
        else { throw ClaudeDesktopConfigError.encryptionFailed }
        root[Self.cacheKey] = blob
        root[Self.uuidKey] = accountUuid
        root.removeValue(forKey: Self.legacyKey)
        let out = try JSONSerialization.data(withJSONObject: root, options: [.withoutEscapingSlashes])
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try out.write(to: fileURL, options: .atomic)
    }
}

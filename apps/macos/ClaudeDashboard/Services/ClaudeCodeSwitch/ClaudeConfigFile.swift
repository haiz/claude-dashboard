import Foundation

/// The `oauthAccount` key of Claude Code's `~/.claude.json`: who Claude Code thinks it
/// is signed in as. Identity metadata only, never a credential.
protocol ClaudeConfigAccountFile {
    /// Canonical JSON of `oauthAccount`; nil when the file or the key is missing.
    func readOAuthAccount() throws -> Data?
    /// Replaces `oauthAccount`, keeping every other key and the file's permissions.
    func writeOAuthAccount(_ json: Data) throws
}

enum ClaudeConfigFileError: Error, Equatable {
    /// The file is missing or is not a JSON object; it is left untouched.
    case unreadable
    /// The value to write is not a JSON object.
    case notAnObject
}

/// `~/.claude.json`. Running Claude Code processes also write this file; the
/// read-modify-rename below keeps that window short (see the spec's error handling).
///
///     let file = ClaudeConfigFile(fileURL: ClaudeConfigFile.defaultURL)
///     try file.writeOAuthAccount(entry.oauthAccount)
struct ClaudeConfigFile: ClaudeConfigAccountFile {
    static var defaultURL: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
    }

    private let fileURL: URL

    init(fileURL: URL) {
        self.fileURL = fileURL
    }

    func readOAuthAccount() throws -> Data? {
        guard let data = try? Data(contentsOf: fileURL),
              let root = OAuthAccountJSON.object(data),
              let account = root["oauthAccount"] else { return nil }
        return OAuthAccountJSON.canonical(account)
    }

    func writeOAuthAccount(_ json: Data) throws {
        guard let account = OAuthAccountJSON.object(json) else { throw ClaudeConfigFileError.notAnObject }
        guard let data = try? Data(contentsOf: fileURL),
              var root = OAuthAccountJSON.object(data) else { throw ClaudeConfigFileError.unreadable }
        let mode = try? FileManager.default.attributesOfItem(atPath: fileURL.path)[.posixPermissions]
        root["oauthAccount"] = account
        let out = try JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .withoutEscapingSlashes])
        try out.write(to: fileURL, options: .atomic)
        if let mode {
            try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: fileURL.path)
        }
    }
}

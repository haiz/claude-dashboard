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
/// A symlinked path is resolved once and the real file is replaced, so the link
/// survives. The replacement is created with the original mode (0600 when unknown)
/// in the same directory, then renamed over the target, so it is never briefly
/// more readable than the file it replaces.
///
///     let file = ClaudeConfigFile(fileURL: ClaudeConfigFile.defaultURL)
///     try file.writeOAuthAccount(entry.oauthAccount)
struct ClaudeConfigFile: ClaudeConfigAccountFile {
    static var defaultURL: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
    }

    private let fileURL: URL

    init(fileURL: URL) {
        self.fileURL = fileURL.resolvingSymlinksInPath()
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
        let attrs = try? FileManager.default.attributesOfItem(atPath: fileURL.path)
        let mode = (attrs?[.posixPermissions] as? NSNumber)?.intValue ?? 0o600
        root["oauthAccount"] = account
        let out = try JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .withoutEscapingSlashes])

        let tmp = fileURL.deletingLastPathComponent()
            .appendingPathComponent(".\(fileURL.lastPathComponent).\(UUID().uuidString).tmp")
        guard FileManager.default.createFile(atPath: tmp.path, contents: out,
                                             attributes: [.posixPermissions: mode]) else {
            try? FileManager.default.removeItem(at: tmp)
            throw CocoaError(.fileWriteUnknown)
        }
        if rename(tmp.path, fileURL.path) != 0 {
            let code = errno
            try? FileManager.default.removeItem(at: tmp)
            throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)
        }
    }
}

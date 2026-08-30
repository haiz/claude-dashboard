import Foundation

/// Reads `~/.claude.json` and returns the `emailAddress` that Claude Code
/// is currently authenticated as. Returns `nil` when the file is missing,
/// malformed, or does not contain an `oauthAccount.emailAddress`.
struct ClaudeCodeAccountDetector {
    private let fileURL: URL

    init(fileURL: URL = FileManager.default
        .homeDirectoryForCurrentUser
        .appendingPathComponent(".claude.json")) {
        self.fileURL = fileURL
    }

    func activeEmail() -> String? {
        guard let data = try? Data(contentsOf: fileURL) else {
            return nil
        }
        do {
            let root = try JSONDecoder().decode(ClaudeConfigRoot.self, from: data)
            return root.oauthAccount?.emailAddress
        } catch {
            print("[ClaudeCodeAccountDetector] Failed to decode \(fileURL.path): \(error)")
            return nil
        }
    }

    private struct ClaudeConfigRoot: Decodable {
        let oauthAccount: OauthAccount?
    }

    private struct OauthAccount: Decodable {
        let emailAddress: String?
    }
}

import Foundation

/// Supported Chromium-based browsers. They differ only in their User Data base path
/// and the Keychain service name holding the Safe Storage password; the cookie
/// decryption mechanism is identical across all of them.
enum Browser: String, Codable, CaseIterable {
    case chrome
    case arc
    case brave
    case edge

    var displayName: String {
        switch self {
        case .chrome: return "Google Chrome"
        case .arc:    return "Arc"
        case .brave:  return "Brave"
        case .edge:   return "Microsoft Edge"
        }
    }

    /// Subdirectory of ~/Library/Application Support containing "Local State" and profiles.
    private var relativeBase: String {
        switch self {
        case .chrome: return "Google/Chrome"
        case .arc:    return "Arc/User Data"
        case .brave:  return "BraveSoftware/Brave-Browser"
        case .edge:   return "Microsoft Edge"
        }
    }

    var basePath: String {
        NSHomeDirectory() + "/Library/Application Support/" + relativeBase
    }

    var keychainService: String {
        switch self {
        case .chrome: return "Chrome Safe Storage"
        case .arc:    return "Arc Safe Storage"
        case .brave:  return "Brave Safe Storage"
        case .edge:   return "Microsoft Edge Safe Storage"
        }
    }
}

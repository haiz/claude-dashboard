import Foundation

/// The origin every claude.ai request is built from, with a loopback-only
/// override used by the helper's transport tests.
///
/// Both `UsageAPIService` and `UsageCommand` read this one type on purpose: two
/// separate readers could point `usage` and `add-key` at different hosts.
///
/// A rejected override falls back to production **silently**. Printing a
/// warning would append text to stderr, and `contract/helper-cli.md` specifies
/// stderr byte for byte.
///
/// This is platform detail, not contract.
enum APIBaseURL {

    static let productionOrigin = "https://claude.ai"
    static let overrideVariable = "CLAUDE_DASHBOARD_API_BASE"

    /// e.g. `https://claude.ai` or `http://127.0.0.1:52341`.
    static var origin: String {
        resolve(ProcessInfo.processInfo.environment[overrideVariable])
    }

    /// The prefix call sites compose paths onto, e.g. `https://claude.ai/api`.
    static var apiRoot: String { origin + "/api" }

    /// Honours only a plain-http loopback origin; everything else is production.
    static func resolve(_ raw: String?) -> String {
        guard let raw,
              let url = URL(string: raw),
              url.scheme == "http",
              let host = url.host,
              host == "127.0.0.1" || host == "localhost" else {
            return productionOrigin
        }
        var origin = "http://\(host)"
        if let port = url.port { origin += ":\(port)" }
        return origin
    }
}

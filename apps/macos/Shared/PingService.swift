import Foundation

/// How one ping ended. Raw values are stored in `ping_logs.outcome` and are never renumbered.
enum PingOutcome: Int, CaseIterable, Sendable {
    case ok = 0
    case expired = 1
    case blocked = 2
    case failed = 3

    var label: String {
        switch self {
        case .ok: return "OK"
        case .expired: return "Session expired"
        case .blocked: return "Blocked by Cloudflare"
        case .failed: return "Failed"
        }
    }
}

/// `detail` says why a ping failed, or notes something about an `.ok` one
/// (the conversation could not be deleted).
struct PingResult: Equatable, Sendable {
    let outcome: PingOutcome
    let detail: String?
}

/// Sends one ping. `PingService` is the real one; tests pass a fake.
protocol Pinging: Sendable {
    func ping(orgId: String, sessionKey: String) async -> PingResult
}

/// Starts an account's usage windows by sending one tiny claude.ai message
/// with its session key: create a conversation, complete one prompt, delete it.
/// The completion endpoint is undocumented; `contract/README.md`, "Ping",
/// records the sequence.
///
///     let result = await PingService().ping(orgId: orgId, sessionKey: key)
///     result.outcome   // .ok
final class PingService: Pinging {
    static let prompt = "Reply with just: ok"
    static let notDeletedDetail = "conversation not deleted"
    private static let completionTimeout: TimeInterval = 60
    private static let requestTimeout: TimeInterval = 30

    private let session: URLSession
    private let baseURL = APIBaseURL.apiRoot

    init(session: URLSession = .shared) {
        self.session = session
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        let id = UUID().uuidString.lowercased()
        let collection = "\(baseURL)/organizations/\(orgId)/chat_conversations"

        let created = await send("POST", collection, body: ["uuid": id, "name": ""],
                                 sessionKey: sessionKey, timeout: Self.requestTimeout)
        if let failure = Self.failure(of: created) { return failure }

        let completion = await send("POST", "\(collection)/\(id)/completion", body: [
            "prompt": Self.prompt,
            "timezone": TimeZone.current.identifier,
            "attachments": [],
            "files": [],
            "rendering_mode": "messages",
        ], sessionKey: sessionKey, timeout: Self.completionTimeout)
        let result = Self.completionResult(completion)

        // Best effort, and also after a failed completion, so no stray chat is left.
        let deleted = await send("DELETE", "\(collection)/\(id)", body: nil,
                                 sessionKey: sessionKey, timeout: Self.requestTimeout)
        if result.outcome == .ok, Self.failure(of: deleted) != nil {
            return PingResult(outcome: .ok, detail: Self.notDeletedDetail)
        }
        return result
    }

    // MARK: - Private

    private typealias Response = Result<(HTTPURLResponse, Data), Error>

    private func send(_ method: String, _ urlString: String, body: [String: Any]?,
                      sessionKey: String, timeout: TimeInterval) async -> Response {
        guard let url = URL(string: urlString) else { return .failure(URLError(.badURL)) }
        var request = URLRequest(url: url, timeoutInterval: timeout)
        request.httpMethod = method
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")
        if let body {
            request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }
        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                return .failure(URLError(.badServerResponse))
            }
            return .success((http, data))
        } catch {
            return .failure(error)
        }
    }

    /// nil when the request succeeded (2xx).
    private static func failure(of response: Response) -> PingResult? {
        switch response {
        case .failure(let error):
            return PingResult(outcome: .failed, detail: error.localizedDescription)
        case .success(let (http, data)):
            let status = http.statusCode
            if (200...299).contains(status) { return nil }
            if status == 401 { return PingResult(outcome: .expired, detail: nil) }
            if status == 403 {
                let outcome: PingOutcome = isCloudflare(http, data) ? .blocked : .expired
                return PingResult(outcome: outcome, detail: nil)
            }
            return PingResult(outcome: .failed, detail: "HTTP \(status)")
        }
    }

    /// A Cloudflare challenge carries `cf-mitigated` or answers with an HTML page;
    /// claude.ai's own 403s are JSON.
    private static func isCloudflare(_ http: HTTPURLResponse, _ data: Data) -> Bool {
        if http.value(forHTTPHeaderField: "cf-mitigated") != nil { return true }
        let head = String(decoding: data.prefix(64), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return head.hasPrefix("<")
    }

    private static func completionResult(_ response: Response) -> PingResult {
        if let failure = failure(of: response) { return failure }
        guard case .success(let (_, data)) = response,
              String(decoding: data, as: UTF8.self).contains("event: message_start") else {
            return PingResult(outcome: .failed, detail: "no reply")
        }
        return PingResult(outcome: .ok, detail: nil)
    }
}

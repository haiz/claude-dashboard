import Foundation

enum UsageAPIError: Error {
    case authExpired
    case httpError(statusCode: Int)
    case invalidResponse
}

struct UsageAPIResult {
    let usage: UsageData
    let newSessionKey: String?
}

struct OrgInfo {
    let uuid: String
    let name: String
    let email: String?
    let capabilities: [String]
    let planHint: AccountPlan?
}

final class UsageAPIService {
    private let session: URLSession
    private let baseURL = "https://claude.ai/api"

    init(session: URLSession = .shared) {
        self.session = session
    }

    // MARK: - Usage

    func fetchUsage(orgId: String, sessionKey: String) async throws -> UsageAPIResult {
        guard let url = URL(string: "\(baseURL)/organizations/\(orgId)/usage") else {
            throw UsageAPIError.invalidResponse
        }
        let request = makeRequest(url: url, sessionKey: sessionKey)

        let (data, response) = try await session.data(for: request)
        let httpResponse = try validateResponse(response)

        let usage = try UsageData.decode(from: data)
        let newSessionKey = parseSessionKey(from: httpResponse)

        return UsageAPIResult(usage: usage, newSessionKey: newSessionKey)
    }

    // MARK: - Organization Info (for email + plan detection)

    func fetchOrganizations(sessionKey: String) async throws -> [OrgInfo] {
        guard let url = URL(string: "\(baseURL)/organizations") else {
            throw UsageAPIError.invalidResponse
        }
        let request = makeRequest(url: url, sessionKey: sessionKey)

        let (data, response) = try await session.data(for: request)
        let _ = try validateResponse(response)

        guard let jsonArray = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return []
        }

        return jsonArray.compactMap { dict in
            guard let uuid = dict["uuid"] as? String,
                  let name = dict["name"] as? String else {
                return nil
            }
            let capabilities = dict["capabilities"] as? [String] ?? []
            let email = dict["email_address"] as? String
                ?? (dict["billing_info"] as? [String: Any])?["email"] as? String
            let planHint = Self.detectPlanTier(from: dict, capabilities: capabilities)
            return OrgInfo(uuid: uuid, name: name, email: email, capabilities: capabilities, planHint: planHint)
        }
    }

    // MARK: - Full Usage

    // The usage endpoint carries no reliable plan-tier signal — `extra_usage` is
    // a pay-as-you-go overage toggle (it flips to is_enabled=false when out of
    // credits) and has no tier/multiplier field. Plan tier comes solely from the
    // organizations endpoint's capabilities (see `detectPlanTier`).
    func fetchFullUsage(orgId: String, sessionKey: String) async throws -> (usage: UsageData, newSessionKey: String?) {
        guard let url = URL(string: "\(baseURL)/organizations/\(orgId)/usage") else {
            throw UsageAPIError.invalidResponse
        }
        let request = makeRequest(url: url, sessionKey: sessionKey)

        let (data, response) = try await session.data(for: request)
        let httpResponse = try validateResponse(response)

        let usage = try UsageData.decode(from: data)
        let newSessionKey = parseSessionKey(from: httpResponse)
        return (usage: usage, newSessionKey: newSessionKey)
    }

    // MARK: - Private

    // Plan tier from the organizations endpoint. The only stable consumer-plan
    // markers the API exposes are the `claude_pro` / `claude_max` capabilities;
    // the 5x vs 20x distinction is not exposed anywhere, so Max falls back to a
    // generic "Max". A consumer chat org without `claude_pro` is treated as Max.
    private static func detectPlanTier(from dict: [String: Any], capabilities: [String]) -> AccountPlan? {
        let caps = Set(capabilities.map { $0.lowercased() })

        // Explicit 5x/20x markers, if the org ever exposes them.
        let jsonString = (try? JSONSerialization.data(withJSONObject: dict))
            .flatMap { String(data: $0, encoding: .utf8) }?.lowercased() ?? ""
        if jsonString.contains("max_20x") || jsonString.contains("max20x") {
            return .max20x
        }
        if jsonString.contains("max_5x") || jsonString.contains("max5x") {
            return .max5x
        }

        // `claude_pro` is the authoritative Pro marker — check it before the chat
        // fallback, since Pro orgs also carry the `chat` capability.
        if caps.contains("claude_pro") { return .pro }
        if caps.contains("claude_max") { return .max200 }

        // A consumer chat org without the Pro marker is Max (tier unknown).
        // Non-consumer orgs (api-only, etc.) are not a plan we display.
        if caps.contains("chat") { return .max200 }
        return nil
    }

    private func makeRequest(url: URL, sessionKey: String) -> URLRequest {
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")
        return request
    }

    private func validateResponse(_ response: URLResponse) throws -> HTTPURLResponse {
        guard let httpResponse = response as? HTTPURLResponse else {
            throw UsageAPIError.invalidResponse
        }

        if httpResponse.statusCode == 401 || httpResponse.statusCode == 403 {
            throw UsageAPIError.authExpired
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            throw UsageAPIError.httpError(statusCode: httpResponse.statusCode)
        }

        return httpResponse
    }

    private func parseSessionKey(from response: HTTPURLResponse) -> String? {
        guard let setCookie = response.value(forHTTPHeaderField: "Set-Cookie") else {
            return nil
        }

        let components = setCookie.components(separatedBy: ";")
        for component in components {
            let trimmed = component.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("sessionKey=") {
                return String(trimmed.dropFirst("sessionKey=".count))
            }
        }

        return nil
    }
}

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

    // MARK: - Full Usage (with plan detection from raw response)

    func fetchFullUsage(orgId: String, sessionKey: String) async throws -> (usage: UsageData, planHint: AccountPlan?, newSessionKey: String?) {
        guard let url = URL(string: "\(baseURL)/organizations/\(orgId)/usage") else {
            throw UsageAPIError.invalidResponse
        }
        let request = makeRequest(url: url, sessionKey: sessionKey)

        let (data, response) = try await session.data(for: request)
        let httpResponse = try validateResponse(response)

        // DEBUG: log raw API response to see seven_day_sonnet field
        if let rawJSON = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            let sonnetValue = rawJSON["seven_day_sonnet"] as Any
            print("[UsageAPI] orgId=\(orgId) seven_day_sonnet=\(sonnetValue)")
        }

        let usage = try UsageData.decode(from: data)
        let newSessionKey = parseSessionKey(from: httpResponse)

        // Detect plan from raw JSON
        // Max plans have extra_usage.is_enabled = true
        var planHint: AccountPlan? = nil
        if let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            if let extraUsage = json["extra_usage"] as? [String: Any],
               let isEnabled = extraUsage["is_enabled"] as? Bool,
               isEnabled {
                // Check for tier/multiplier in extra_usage to distinguish Max 5x vs 20x
                if let tier = extraUsage["tier"] as? String {
                    if tier.contains("20x") { planHint = .max20x }
                    else if tier.contains("5x") { planHint = .max5x }
                    else { planHint = .max200 }
                } else if let multiplier = extraUsage["multiplier"] as? Int {
                    if multiplier >= 20 { planHint = .max20x }
                    else if multiplier >= 5 { planHint = .max5x }
                    else { planHint = .max200 }
                } else {
                    planHint = .max200
                }
            } else if json.keys.contains("extra_usage") {
                // extra_usage is null or is_enabled is false → Pro plan
                planHint = .pro
            }
            // If extra_usage key is absent entirely, leave planHint nil (unknown)
        }

        return (usage: usage, planHint: planHint, newSessionKey: newSessionKey)
    }

    // MARK: - Private

    private static func detectPlanTier(from dict: [String: Any], capabilities: [String]) -> AccountPlan? {
        // Serialize the full org JSON to a string and search for tier patterns
        let jsonString = (try? JSONSerialization.data(withJSONObject: dict))
            .flatMap { String(data: $0, encoding: .utf8) }?.lowercased() ?? ""

        // Look for explicit 5x/20x markers anywhere in the org JSON
        if jsonString.contains("max_20x") || jsonString.contains("max20x") {
            return .max20x
        }
        if jsonString.contains("max_5x") || jsonString.contains("max5x") {
            return .max5x
        }

        // Check capabilities for known patterns
        let capsJoined = capabilities.joined(separator: " ").lowercased()
        if capsJoined.contains("max") || capsJoined.contains("extra_usage") {
            return .max200  // Max but can't determine 5x/20x
        }

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

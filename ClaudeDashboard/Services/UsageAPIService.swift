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

final class UsageAPIService {
    private let session: URLSession
    private let baseURL = "https://claude.ai/api/organizations"

    init(session: URLSession = .shared) {
        self.session = session
    }

    func fetchUsage(orgId: String, sessionKey: String) async throws -> UsageAPIResult {
        let url = URL(string: "\(baseURL)/\(orgId)/usage")!
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")

        let (data, response) = try await session.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw UsageAPIError.invalidResponse
        }

        if httpResponse.statusCode == 401 || httpResponse.statusCode == 403 {
            throw UsageAPIError.authExpired
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            throw UsageAPIError.httpError(statusCode: httpResponse.statusCode)
        }

        let usage = try UsageData.decode(from: data)
        let newSessionKey = parseSessionKey(from: httpResponse)

        return UsageAPIResult(usage: usage, newSessionKey: newSessionKey)
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

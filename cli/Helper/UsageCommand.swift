import Foundation

enum UsageCommand {

    static func run(args: [String]) -> Int32 {
        guard args.count >= 2 else {
            fputs("Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n", stderr)
            return 1
        }

        let orgId = args[0]
        let sessionKey = args[1]

        guard let url = URL(string: "https://claude.ai/api/organizations/\(orgId)/usage") else {
            fputs("Invalid orgId.\n", stderr)
            return 1
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")

        let semaphore = DispatchSemaphore(value: 0)
        var resultData: Data?
        var resultStatus: Int = 0
        var resultError: Error?

        let task = URLSession.shared.dataTask(with: request) { data, response, error in
            resultData = data
            resultError = error
            if let http = response as? HTTPURLResponse {
                resultStatus = http.statusCode
            }
            semaphore.signal()
        }
        task.resume()

        if semaphore.wait(timeout: .now() + 15) == .timedOut {
            task.cancel()
            fputs("Request timed out.\n", stderr)
            return 1
        }

        if let error = resultError {
            fputs("Network error: \(error.localizedDescription)\n", stderr)
            return 1
        }

        guard (200...299).contains(resultStatus) else {
            fputs("HTTP \(resultStatus)\n", stderr)
            return 1
        }

        guard let data = resultData,
              let body = String(data: data, encoding: .utf8) else {
            fputs("Empty response.\n", stderr)
            return 1
        }

        print(body)
        return 0
    }
}

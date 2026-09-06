import Foundation

enum UsageCommand {

    static func run(args: [String]) -> Int32 {
        guard args.count >= 2 else {
            fputs("Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n", stderr)
            return 1
        }

        let orgId = args[0]
        let sessionKey = args[1]

        // On this Foundation, `URL(string:)` percent-encodes whitespace and
        // control characters into the path instead of returning nil for
        // them, so it cannot be relied on alone to catch a malformed orgId
        // (verified: a request for an orgId containing a space or a slash
        // actually reaches the server and gets a 404, rather than being
        // rejected up front). Validated explicitly here, mirroring
        // apps/linux/core/src/api.rs's `validate_org_id`: reject empty, or
        // any Unicode scalar that is `/`, `?`, `#`, a control character
        // (Unicode general category Cc, matching Rust's `char::is_control`),
        // or whitespace (Unicode White_Space property, matching Rust's
        // `char::is_whitespace`) — checked scalar-by-scalar, as Rust checks
        // `char`-by-`char`, rather than by Swift's grapheme-cluster
        // `Character`.
        guard !orgId.isEmpty, !orgId.unicodeScalars.contains(where: { scalar in
            scalar == "/" || scalar == "?" || scalar == "#"
                || scalar.properties.generalCategory == .control
                || scalar.properties.isWhitespace
        }) else {
            fputs("Invalid orgId.\n", stderr)
            return 1
        }

        guard let url = URL(string: "\(APIBaseURL.apiRoot)/organizations/\(orgId)/usage") else {
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

        // The decoded String is the contract's non-UTF8 gate and nothing else:
        // what gets written is `data`, the upstream bytes themselves.
        guard let data = resultData, !data.isEmpty,
              String(data: data, encoding: .utf8) != nil else {
            fputs("Empty response.\n", stderr)
            return 1
        }

        // `print` would append a newline; contract/helper-cli.md "usage"
        // specifies the upstream body byte-for-byte, and apps/linux's
        // `usage.rs` uses `print!` for the same reason.
        //
        // `try? write(contentsOf:)` rather than `write(_:)`: the ObjC
        // `write(_:)` raises an uncatchable NSException on a failed write
        // (EBADF, ENOSPC on a redirected stdout, EIO) where `print` swallowed
        // it. This is not about broken pipes -- measured, `print` and
        // `write(contentsOf:)` behave identically there, both dying with
        // SIGPIPE above the 64KB pipe buffer and both fine below it.
        try? FileHandle.standardOutput.write(contentsOf: data)
        return 0
    }
}

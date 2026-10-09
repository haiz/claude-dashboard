import Foundation
import Network

enum OAuthCallbackError: Error, Equatable {
    case timedOut
    case cancelled
    case listenerFailed
}

/// A one-shot loopback HTTP server for the OAuth redirect. It binds `127.0.0.1` on an
/// OS-chosen port, waits for exactly one `/callback?code=...&state=...` whose `state`
/// matches, serves a plain "done" page to the browser, and hands the code back.
///
/// A callback whose `state` does not match is answered and ignored (it is not our grant),
/// so a stray hit cannot resolve the wait. `cancel()` and the timeout both wake the waiter.
///
///     let listener = OAuthCallbackListener(expectedState: pkce.state)
///     let port = try listener.start()
///     // redirect_uri = "http://127.0.0.1:\(port)/callback"
///     let code = try await listener.waitForCode(timeout: 300)
///     listener.cancel()
final class OAuthCallbackListener {
    private let expectedState: String
    private let queue = DispatchQueue(label: "oauth-callback-listener")
    private var listener: NWListener?

    /// Guards `continuation`/`settled` so the timeout, a callback and cancel resolve once.
    private let lock = NSLock()
    private var continuation: CheckedContinuation<String, Error>?
    private var settled = false

    init(expectedState: String) {
        self.expectedState = expectedState
    }

    /// Binds a loopback port and begins accepting. Returns the chosen port.
    func start() throws -> Int {
        let params = NWParameters.tcp
        params.requiredInterfaceType = .loopback
        let listener = try NWListener(using: params)
        self.listener = listener

        listener.newConnectionHandler = { [weak self] connection in
            self?.handle(connection)
        }
        let ready = DispatchSemaphore(value: 0)
        var startError: Error?
        listener.stateUpdateHandler = { state in
            switch state {
            case .ready: ready.signal()
            case .failed(let error): startError = error; ready.signal()
            default: break
            }
        }
        listener.start(queue: queue)

        guard ready.wait(timeout: .now() + 5) == .success, startError == nil,
              let port = listener.port?.rawValue else {
            listener.cancel()
            throw OAuthCallbackError.listenerFailed
        }
        return Int(port)
    }

    /// Waits for the matching callback. Throws `.timedOut`, or `.cancelled` if `cancel()`
    /// is called first.
    func waitForCode(timeout: TimeInterval) async throws -> String {
        let timeoutItem = DispatchWorkItem { [weak self] in self?.settle(.failure(.timedOut)) }
        queue.asyncAfter(deadline: .now() + timeout, execute: timeoutItem)
        defer { timeoutItem.cancel() }
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if settled {
                lock.unlock()
                continuation.resume(throwing: OAuthCallbackError.cancelled)
                return
            }
            self.continuation = continuation
            lock.unlock()
        }
    }

    /// Stops the server and, if nothing arrived, fails the waiter with `.cancelled`.
    func cancel() {
        settle(.failure(.cancelled))
    }

    private func handle(_ connection: NWConnection) {
        connection.start(queue: queue)
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, _, _ in
            guard let self else { connection.cancel(); return }
            let request = data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
            let code = self.matchingCode(in: request)
            self.respond(connection, found: code != nil)
            if let code { self.settle(.success(code)) }
        }
    }

    /// The `code` from the request line `GET /callback?code=...&state=...`, only when
    /// `state` equals the one we are waiting for.
    private func matchingCode(in request: String) -> String? {
        guard let line = request.split(separator: "\r\n").first ?? request.split(separator: "\n").first,
              let path = line.split(separator: " ").dropFirst().first,
              let c = URLComponents(string: String(path)),
              c.path == "/callback" else { return nil }
        let items = c.queryItems ?? []
        guard items.first(where: { $0.name == "state" })?.value == expectedState else { return nil }
        return items.first { $0.name == "code" }?.value
    }

    private func respond(_ connection: NWConnection, found: Bool) {
        let title = found ? "Signed in" : "Waiting"
        let body = found
            ? "Claude Code is now linked. You can close this tab and return to Claude Dashboard."
            : "Claude Dashboard is still waiting. You can close this tab."
        let html = "<!doctype html><meta charset=utf-8><title>Claude Dashboard</title>"
            + "<body style=\"font:16px -apple-system;margin:4rem auto;max-width:28rem;text-align:center\">"
            + "<h2>\(title)</h2><p>\(body)</p></body>"
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n"
            + "Content-Length: \(html.utf8.count)\r\nConnection: close\r\n\r\n\(html)"
        connection.send(content: Data(response.utf8), completion: .contentProcessed { _ in
            connection.cancel()
        })
    }

    private func settle(_ result: Result<String, OAuthCallbackError>) {
        lock.lock()
        if settled { lock.unlock(); return }
        settled = true
        let continuation = self.continuation
        self.continuation = nil
        lock.unlock()

        listener?.cancel()
        listener = nil
        switch result {
        case .success(let code): continuation?.resume(returning: code)
        case .failure(let error): continuation?.resume(throwing: error)
        }
    }
}

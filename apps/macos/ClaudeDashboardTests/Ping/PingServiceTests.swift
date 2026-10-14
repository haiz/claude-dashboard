import XCTest
@testable import ClaudeDashboard

final class PingServiceTests: XCTestCase {

    /// One request as the mock saw it. Bodies are read from the stream because
    /// URLSession moves `httpBody` into `httpBodyStream` before a URLProtocol sees it.
    private struct Seen {
        let method: String
        let path: String
        let cookie: String?
        let body: [String: Any]
    }

    private final class Recorder: @unchecked Sendable {
        private let lock = NSLock()
        private var items: [Seen] = []
        func append(_ seen: Seen) { lock.lock(); items.append(seen); lock.unlock() }
        var all: [Seen] { lock.lock(); defer { lock.unlock() }; return items }
    }

    private var recorder: Recorder!
    private var service: PingService!

    override func setUp() {
        super.setUp()
        recorder = Recorder()
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        service = PingService(session: URLSession(configuration: config))
    }

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    private static func body(of request: URLRequest) -> [String: Any] {
        var data = request.httpBody ?? Data()
        if data.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let n = stream.read(&buffer, maxLength: buffer.count)
                if n <= 0 { break }
                data.append(buffer, count: n)
            }
        }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    }

    /// Routes by step: create (POST, no `/completion`), completion, delete.
    private func respond(
        create: (Int, [String: String], String) = (201, [:], "{}"),
        completion: (Int, [String: String], String) = (200, [:], "event: message_start\ndata: {}\n\nevent: message_stop\ndata: {}\n\n"),
        delete: (Int, [String: String], String) = (204, [:], "")
    ) {
        let recorder = recorder!
        MockURLProtocol.requestHandler = { request in
            let method = request.httpMethod ?? "GET"
            let path = request.url?.path ?? ""
            recorder.append(Seen(method: method, path: path,
                                 cookie: request.value(forHTTPHeaderField: "Cookie"),
                                 body: Self.body(of: request)))
            let reply: (Int, [String: String], String)
            if method == "DELETE" {
                reply = delete
            } else if path.hasSuffix("/completion") {
                reply = completion
            } else {
                reply = create
            }
            let response = HTTPURLResponse(url: request.url!, statusCode: reply.0,
                                           httpVersion: nil, headerFields: reply.1)!
            return (response, Data(reply.2.utf8))
        }
    }

    func testOkPingCreatesCompletesAndDeletesTheConversation() async {
        respond()

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .ok, detail: nil))
        let seen = recorder.all
        XCTAssertEqual(seen.map(\.method), ["POST", "POST", "DELETE"])
        XCTAssertEqual(seen[0].path, "/api/organizations/org-1/chat_conversations")
        let id = seen[0].body["uuid"] as? String
        XCTAssertNotNil(id)
        XCTAssertEqual(seen[0].body["name"] as? String, "")
        XCTAssertEqual(seen[1].path, "/api/organizations/org-1/chat_conversations/\(id!)/completion")
        XCTAssertEqual(seen[1].body["prompt"] as? String, PingService.prompt)
        XCTAssertEqual(seen[1].body["rendering_mode"] as? String, "messages")
        XCTAssertNil(seen[1].body["model"])
        XCTAssertEqual(seen[2].path, "/api/organizations/org-1/chat_conversations/\(id!)")
        XCTAssertTrue(seen.allSatisfy { $0.cookie == "sessionKey=sk-1" })
    }

    func testFailedDeleteStillCountsAsOkWithANote() async {
        respond(delete: (500, [:], ""))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .ok, detail: PingService.notDeletedDetail))
    }

    func test401OnCreateIsExpiredAndStopsThere() async {
        respond(create: (401, [:], "{}"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .expired)
        XCTAssertEqual(recorder.all.count, 1)
    }

    func test403WithJsonBodyIsExpired() async {
        respond(create: (403, [:], #"{"error":{"type":"permission_error"}}"#))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .expired)
    }

    func test403WithCfMitigatedHeaderIsBlocked() async {
        respond(create: (403, ["cf-mitigated": "challenge"], "{}"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .blocked)
    }

    func test403WithHtmlBodyIsBlocked() async {
        respond(create: (403, [:], "  <!DOCTYPE html><title>Just a moment...</title>"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .blocked)
    }

    func test429OnCompletionFailsAndStillDeletesTheConversation() async {
        respond(completion: (429, [:], #"{"error":{"type":"rate_limit_error"}}"#))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .failed, detail: "HTTP 429"))
        XCTAssertEqual(recorder.all.map(\.method), ["POST", "POST", "DELETE"])
    }

    func testStreamWithoutMessageStartFails() async {
        respond(completion: (200, [:], "event: ping\ndata: {}\n\n"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .failed, detail: "no reply"))
    }

    func testNetworkErrorFails() async {
        MockURLProtocol.requestHandler = { _ in throw URLError(.notConnectedToInternet) }

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .failed)
        XCTAssertNotNil(result.detail)
    }
}

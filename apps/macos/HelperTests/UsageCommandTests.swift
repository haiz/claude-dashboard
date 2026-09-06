import XCTest

/// `usage` driven as a real process against a loopback server.
/// `contract/helper-cli.md` "usage" is the source of truth for every string here.
final class UsageCommandTests: XCTestCase {

    private var server: LoopbackServer!

    override func setUp() {
        super.setUp()
        server = LoopbackServer()
        server.start()
    }

    override func tearDown() {
        server.stop()
        server = nil
        super.tearDown()
    }

    private func runUsage(orgId: String = "org-1", key: String = "sk-fake-test-key") -> HelperProcess.Result {
        HelperProcess.run(
            ["usage", orgId, key],
            env: [APIBaseURL.overrideVariable: server.origin]
        )
    }

    /// The passthrough rule, plus the four headers the contract specifies —
    /// asserted against what actually arrived on the socket.
    func testSuccessfulRequestIsPassedThroughAndCarriesTheContractHeaders() throws {
        let body = #"{"five_hour":{"utilization":12},"unknown_field":"kept"}"#
        server.respond(path: LoopbackServer.usageWildcard, with: .json(body))

        let result = runUsage(orgId: "org-abc")

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertEqual(result.stdoutText, body)
        XCTAssertEqual(result.stderr, "")

        let request = try XCTUnwrap(server.recorded.first)
        XCTAssertEqual(request.method, "GET")
        XCTAssertEqual(request.path, "/api/organizations/org-abc/usage")
        XCTAssertEqual(request.headers["accept"], "*/*")
        XCTAssertEqual(request.headers["content-type"], "application/json")
        XCTAssertEqual(request.headers["anthropic-client-platform"], "web_claude_ai")
        XCTAssertEqual(request.headers["cookie"], "sessionKey=sk-fake-test-key")
    }
}

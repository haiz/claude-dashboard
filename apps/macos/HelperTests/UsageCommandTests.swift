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

    // MARK: - Body shapes

    func testEmptyBodyIsReportedAsEmptyResponse() {
        server.respond(path: LoopbackServer.usageWildcard,
                       with: .reply(status: 200, headers: [:], body: Data()))

        let result = runUsage()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "Empty response.\n")
        XCTAssertEqual(result.stdout, Data())
    }

    func testNonUTF8BodyIsReportedAsEmptyResponse() {
        // 0xff 0xfe is not valid UTF-8 in any position.
        server.respond(path: LoopbackServer.usageWildcard,
                       with: .reply(status: 200, headers: [:], body: Data([0xff, 0xfe])))

        let result = runUsage()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "Empty response.\n")
    }

    // MARK: - Statuses

    /// Ruling B: the helper never maps 401/403 to an expired account the way
    /// the GUI does — it prints the real status.
    func test401PrintsTheStatusVerbatim() {
        server.respond(path: LoopbackServer.usageWildcard,
                       with: .reply(status: 401, headers: [:], body: Data("nope".utf8)))

        let result = runUsage()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "HTTP 401\n")
    }

    func test500PrintsTheStatusVerbatim() {
        server.respond(path: LoopbackServer.usageWildcard,
                       with: .reply(status: 500, headers: [:], body: Data()))

        let result = runUsage()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "HTTP 500\n")
    }

    // MARK: - Transport failures

    func testConnectionClosedMidRequestIsANetworkError() {
        server.respond(path: LoopbackServer.usageWildcard, with: .closeImmediately)

        let result = runUsage()

        XCTAssertEqual(result.exitCode, 1)
        // Only the prefix is contract; the rest is localizedDescription.
        XCTAssertTrue(result.stderr.hasPrefix("Network error:"),
                      "unexpected stderr: \(result.stderr)")
    }

    /// The 15-second case. The server must hold the connection open: closing it
    /// would produce a network error instead of the client's own timeout.
    func testSilentServerTimesOutAfterFifteenSeconds() {
        server.respond(path: LoopbackServer.usageWildcard, with: .staySilent)

        let started = Date()
        let result = runUsage()
        let elapsed = Date().timeIntervalSince(started)

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "Request timed out.\n")
        XCTAssertGreaterThan(elapsed, 14, "the timeout fired far too early to be the real one")
    }

    // MARK: - Argument handling

    func testMissingArgumentsPrintTheUsageBanner() {
        let result = HelperProcess.run(["usage", "org-1"],
                                       env: [APIBaseURL.overrideVariable: server.origin])

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr,
                       "Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n")
        XCTAssertTrue(server.recorded.isEmpty, "nothing should reach the network")
    }

    // NOTE: a ninth case, `testOrgIdWithWhitespaceIsRejectedBeforeAnyRequest`
    // ("Invalid orgId." for a whitespace-containing orgId), was withdrawn.
    // On this toolchain (macOS 26.5.2), `URL(string:)` no longer returns nil
    // for a whitespace- or control-character-laden path component — it
    // percent-encodes it instead (verified: "bad id" -> .../bad%20id/usage,
    // and every other tried character behaves the same). The guard in
    // `UsageCommand.swift` that maps a nil URL to "Invalid orgId." is
    // therefore unreachable via orgId content on this platform, so the case
    // as specified cannot pass without either enshrining a real request as
    // correct behavior or adding orgId validation and amending
    // `contract/helper-cli.md` — both out of scope for this task. See the
    // task-5 report for the full evidence; this needs a controller ruling.
}

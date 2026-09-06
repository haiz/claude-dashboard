import XCTest

/// `add-key` driven as a real process: stdin, two endpoints, and a store write.
/// `contract/helper-cli.md` "add-key" is the source of truth for every string,
/// and `contract/cases/manual-key.json` for what the repair branch may write.
final class AddKeyCommandTests: XCTestCase {

    private var server: LoopbackServer!
    private var suite: String!

    override func setUp() {
        super.setUp()
        server = LoopbackServer()
        server.start()
        suite = StoreFixture.makeSuiteName()
    }

    override func tearDown() {
        server.stop()
        server = nil
        StoreFixture.destroy(suite: suite)
        super.tearDown()
    }

    private func runAddKey(stdin: String) -> HelperProcess.Result {
        HelperProcess.run(
            ["add-key"],
            stdin: stdin,
            env: [
                APIBaseURL.overrideVariable: server.origin,
                HelperAccountStore.suiteVariable: suite
            ]
        )
    }

    /// A JSON array literal. Interpolating `[String]` directly would emit
    /// Swift's array description with escaped quotes, not JSON.
    private func jsonArray(_ values: [String]) -> String {
        "[" + values.map { "\"\($0)\"" }.joined(separator: ",") + "]"
    }

    private func accountBody(uuid: String, email: String?, orgUuid: String,
                             capabilities: [String] = ["chat"]) -> String {
        let emailField = email.map { "\"email_address\":\"\($0)\"," } ?? ""
        return """
        {\(emailField)"uuid":"\(uuid)","memberships":[
          {"role":"admin","organization":{"uuid":"\(orgUuid)","name":"Org",
           "capabilities":\(jsonArray(capabilities))}}
        ]}
        """
    }

    private func orgsBody(uuid: String, capabilities: [String]) -> String {
        """
        [{"uuid":"\(uuid)","name":"Org","capabilities":\(jsonArray(capabilities))}]
        """
    }

    // MARK: - Rejections

    func testEmptyStdin() {
        let result = runAddKey(stdin: "   \n")

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "No session key on stdin.\n")
        XCTAssertTrue(server.recorded.isEmpty, "nothing should reach the network")
    }

    func testAccountEndpointRejectsTheKey() {
        server.respond(path: "/api/account", with: .reply(status: 401, headers: [:], body: Data()))

        let result = runAddKey(stdin: "sk-fake-expired-key\n")

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "Session key not accepted (expired or invalid).\n")
        XCTAssertEqual(StoreFixture.read(fromSuite: suite), [])
    }

    func testAddWithNoChatOrg() {
        server.respond(path: "/api/account",
                       with: .json(accountBody(uuid: "acct-1", email: "me@example.com",
                                               orgUuid: "org-1", capabilities: ["raven"])))
        server.respond(path: "/api/organizations",
                       with: .json(orgsBody(uuid: "org-1", capabilities: ["raven"])))

        let result = runAddKey(stdin: "sk-fake-test-key\n")

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "No organization with chat access.\n")
        XCTAssertEqual(StoreFixture.read(fromSuite: suite), [])
    }

    // MARK: - Add

    func testAddStoresTheAccountWithAnEncryptedKey() {
        let key = "sk-fake-test-key"
        server.respond(path: "/api/account",
                       with: .json(accountBody(uuid: "acct-1", email: "me@example.com",
                                               orgUuid: "org-1")))
        server.respond(path: "/api/organizations",
                       with: .json(orgsBody(uuid: "org-1", capabilities: ["chat", "claude_pro"])))

        let result = runAddKey(stdin: "\(key)\n")

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertEqual(result.stderr, "Added: me@example.com (Pro)\n")
        XCTAssertFalse(result.stderr.contains(key), "the session key must never reach stderr")

        let stored = StoreFixture.read(fromSuite: suite)
        XCTAssertEqual(stored.count, 1)
        XCTAssertEqual(stored[0].accountUuid, "acct-1")
        XCTAssertEqual(stored[0].email, "me@example.com")
        XCTAssertEqual(stored[0].orgId, "org-1")
        XCTAssertEqual(stored[0].plan, .pro)
        XCTAssertEqual(stored[0].source, .manual)
        XCTAssertEqual(stored[0].status, .active)
        XCTAssertNotEqual(stored[0].sessionKey, key, "the key must be stored encrypted")
        XCTAssertEqual(CryptoService.decrypt(stored[0].sessionKey ?? ""), key)

        // The key travelled as a cookie on both calls, on the wire.
        XCTAssertEqual(server.recorded.count, 2)
        for request in server.recorded {
            XCTAssertEqual(request.headers["cookie"], "sessionKey=\(key)")
        }
    }

    // MARK: - Repair

    func testRepairRewritesOnlyThePermittedFields() {
        let key = "sk-fake-new-key"
        let existing = StoreFixture.account(
            name: "me@example.com", email: "me@example.com", orgId: "org-1",
            accountUuid: "acct-1", sessionKey: "OLD-STORED-VALUE", plan: .pro)
        StoreFixture.seed([existing], intoSuite: suite)

        server.respond(path: "/api/account",
                       with: .json(accountBody(uuid: "acct-1", email: "me@example.com",
                                               orgUuid: "org-1")))
        server.respond(path: "/api/organizations",
                       with: .json(orgsBody(uuid: "org-1", capabilities: ["chat", "claude_pro"])))

        let result = runAddKey(stdin: "\(key)\n")

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertEqual(result.stderr, "Updated key: me@example.com\n")
        XCTAssertFalse(result.stderr.contains(key), "the session key must never reach stderr")

        let stored = StoreFixture.read(fromSuite: suite)
        XCTAssertEqual(stored.count, 1, "a repair must not add a second record")
        // Differs from both the old stored value and the plaintext: proving it
        // differs from only one of the two would prove nothing about encryption.
        XCTAssertNotEqual(stored[0].sessionKey, "OLD-STORED-VALUE")
        XCTAssertNotEqual(stored[0].sessionKey, key)
        XCTAssertEqual(CryptoService.decrypt(stored[0].sessionKey ?? ""), key)
        // Untouched fields.
        XCTAssertEqual(stored[0].id, existing.id)
        XCTAssertEqual(stored[0].orgId, "org-1")
        XCTAssertEqual(stored[0].source, .browser, "a key never changes a record's source")
        XCTAssertEqual(stored[0].plan, .pro)
    }

    func testRepairReportsAChangedPlan() {
        let existing = StoreFixture.account(
            name: "me@example.com", email: "me@example.com", orgId: "org-1",
            accountUuid: "acct-1", sessionKey: "OLD-STORED-VALUE", plan: .pro)
        StoreFixture.seed([existing], intoSuite: suite)

        server.respond(path: "/api/account",
                       with: .json(accountBody(uuid: "acct-1", email: "me@example.com",
                                               orgUuid: "org-1")))
        // claude_max without claude_pro resolves to the generic Max tier.
        server.respond(path: "/api/organizations",
                       with: .json(orgsBody(uuid: "org-1", capabilities: ["chat", "claude_max"])))

        let result = runAddKey(stdin: "sk-fake-new-key\n")

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertEqual(result.stderr,
                       "Updated key: me@example.com\nUpdated plan: me@example.com (Pro -> Max)\n")
        XCTAssertEqual(StoreFixture.read(fromSuite: suite)[0].plan, .max200)
    }
}

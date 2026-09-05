import XCTest

/// Drives `sync` through an injected environment. Nothing here reaches a
/// browser, the Keychain, UserDefaults or the network: `contract/helper-cli.md`
/// is the source of truth for every line and exit code asserted below.
final class SyncCommandTests: XCTestCase {

    // MARK: - Fixtures

    private func makeCandidate(
        displayName: String,
        sessionKey: String?,
        orgId: String?
    ) -> (profile: BrowserProfile, cookies: ChromeCookieResult) {
        (
            profile: BrowserProfile(
                path: "Default",
                displayName: displayName,
                googleEmail: "",
                browser: .chrome
            ),
            cookies: ChromeCookieResult(sessionKey: sessionKey, orgId: orgId)
        )
    }

    private func makeAccount(
        email: String,
        orgId: String?,
        plan: AccountPlan
    ) -> Account {
        Account(
            id: UUID(),
            name: email,
            email: email,
            chromeProfilePath: "Default",
            chromeProfileName: nil,
            orgId: orgId,
            accountUuid: "acct-1",
            sessionKey: "encrypted-key",
            browser: .chrome,
            plan: plan,
            lastSynced: Date(timeIntervalSince1970: 1_000_000),
            status: .active,
            source: .browser
        )
    }

    /// Collects what the command emits and what it would persist.
    private final class Recorder {
        var lines: [String] = []
        var saved: [Account]?
    }

    private func makeEnvironment(
        candidates: [(profile: BrowserProfile, cookies: ChromeCookieResult)],
        accounts: [Account],
        recorder: Recorder,
        handler: ((URLRequest) throws -> (HTTPURLResponse, Data))? = nil
    ) -> SyncCommand.Environment {
        MockURLProtocol.requestHandler = handler
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]

        return SyncCommand.Environment(
            candidates: { candidates },
            apiService: UsageAPIService(session: URLSession(configuration: config)),
            loadAccounts: { accounts },
            saveAccounts: { recorder.saved = $0 },
            log: { recorder.lines.append($0) }
        )
    }

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    // MARK: - Tests

    /// `contract/helper-cli.md` "sync": the one failure case. No profile with a
    /// session at all is an error for the whole run, not a skip.
    func testNoCandidateProfilesExitsOneWithGuidance() async {
        let recorder = Recorder()
        let env = makeEnvironment(candidates: [], accounts: [], recorder: recorder)

        let code = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(code, 1)
        XCTAssertEqual(recorder.lines, [
            "Scanning installed browsers for Claude sessions...",
            "No browser profiles found with active Claude sessions.",
            "Make sure you're logged into claude.ai in a supported browser."
        ])
        XCTAssertNil(recorder.saved, "a failed scan must not write the store")
    }

    // MARK: - Heal path (contract/cases/plan-refresh.json)

    /// `/api/account` always succeeds; `/api/organizations` answers with
    /// `orgsBody`, or with `status` and an empty body when it is nil.
    private func orgsHandler(
        status: Int = 200,
        orgsBody: String?
    ) -> (URLRequest) throws -> (HTTPURLResponse, Data) {
        { request in
            let path = request.url?.path ?? ""
            if path == "/api/organizations" {
                guard let orgsBody else {
                    return (HTTPURLResponse(url: request.url!, statusCode: status,
                                            httpVersion: nil, headerFields: nil)!, Data())
                }
                return (HTTPURLResponse(url: request.url!, statusCode: 200,
                                        httpVersion: nil, headerFields: nil)!,
                        Data(orgsBody.utf8))
            }
            let account = #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[]}"#
            return (HTTPURLResponse(url: request.url!, statusCode: 200,
                                    httpVersion: nil, headerFields: nil)!,
                    Data(account.utf8))
        }
    }

    private static let proOrg =
        #"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_pro"]}]"#

    /// `contract/helper-cli.md` "sync": the heal line is printed immediately
    /// after that profile's skip line.
    func testHealWritesThePlanAndReportsItRightAfterTheSkipLine() async throws {
        let recorder = Recorder()
        let stored = makeAccount(email: "person@example.com", orgId: "org-1", plan: .max200)
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [stored],
            recorder: recorder,
            handler: orgsHandler(orgsBody: Self.proOrg))

        let code = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(code, 0)
        XCTAssertEqual(recorder.saved?.first?.plan, .pro)
        let skipIndex = try XCTUnwrap(
            recorder.lines.firstIndex(of: "  Skipping Person 2 (already added)"))
        // dropFirst, not [skipIndex + 1]: a mutation that removes the heal line
        // would trap on an index, and a crash cannot be told apart from a test
        // that is simply broken.
        XCTAssertEqual(recorder.lines.dropFirst(skipIndex + 1).first,
                       "  Updated plan: Person 2 (Max -> Pro)")
    }

    /// Rule 1 of `contract/cases/plan-refresh.json`: a blip must not overwrite
    /// a known-good tier.
    func testHealLeavesThePlanAloneWhenOrganizationsFails() async {
        let recorder = Recorder()
        let stored = makeAccount(email: "person@example.com", orgId: "org-1", plan: .max20x)
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [stored],
            recorder: recorder,
            handler: orgsHandler(status: 500, orgsBody: nil))

        _ = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(recorder.saved?.first?.plan, .max20x)
        XCTAssertFalse(recorder.lines.contains { $0.hasPrefix("  Updated plan:") })
    }

    /// The e2e run of 2026-09-04 proved this by diffing the real account store;
    /// this is that check as a regression guard.
    func testHealWritesNothingButThePlan() async throws {
        let recorder = Recorder()
        let stored = makeAccount(email: "person@example.com", orgId: "org-1", plan: .max200)
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [stored],
            recorder: recorder,
            handler: orgsHandler(orgsBody: Self.proOrg))

        _ = await SyncCommand.runAsync(env: env)

        let after = try XCTUnwrap(recorder.saved?.first)
        XCTAssertEqual(after.plan, .pro, "sanity: this run must have healed")
        XCTAssertEqual(after.lastSynced, stored.lastSynced)
        XCTAssertEqual(after.status, stored.status)
        XCTAssertEqual(after.sessionKey, stored.sessionKey)
        XCTAssertEqual(after.email, stored.email)
        XCTAssertEqual(after.name, stored.name)
        XCTAssertEqual(after.accountUuid, stored.accountUuid)
    }

    // MARK: - Add path

    private static let maxOrg =
        #"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_max"]}]"#

    private static let accountWithOrg1 =
        #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[{"organization":{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_max"]}}]}"#

    /// A session no stored account matches is added, named by e-mail, with the
    /// tier of the org `resolveOrgId` picked.
    func testNewAccountIsAddedWithThePlanOfItsOrg() async {
        let recorder = Recorder()
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [],
            recorder: recorder,
            handler: { request in
                let body = (request.url?.path ?? "") == "/api/organizations"
                    ? Self.maxOrg
                    : Self.accountWithOrg1
                return (HTTPURLResponse(url: request.url!, statusCode: 200,
                                        httpVersion: nil, headerFields: nil)!,
                        Data(body.utf8))
            })

        let code = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(code, 0)
        XCTAssertEqual(recorder.saved?.count, 1)
        XCTAssertEqual(recorder.saved?.first?.name, "person@example.com")
        XCTAssertEqual(recorder.saved?.first?.plan, .max200)
        XCTAssertTrue(recorder.lines.contains("  Added: person@example.com (Max)"))
        XCTAssertTrue(recorder.lines.contains("Synced 1 account(s) successfully."))
    }

    /// `contract/helper-cli.md` "sync": a failed `/api/organizations` does not
    /// skip the candidate — session validity came from `/api/account`. It only
    /// leaves the plan at the add-time fallback.
    func testNewAccountIsStillAddedWhenOrganizationsFails() async {
        let recorder = Recorder()
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [],
            recorder: recorder,
            handler: { request in
                if (request.url?.path ?? "") == "/api/organizations" {
                    return (HTTPURLResponse(url: request.url!, statusCode: 500,
                                            httpVersion: nil, headerFields: nil)!, Data())
                }
                return (HTTPURLResponse(url: request.url!, statusCode: 200,
                                        httpVersion: nil, headerFields: nil)!,
                        Data(Self.accountWithOrg1.utf8))
            })

        _ = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(recorder.saved?.count, 1)
        XCTAssertEqual(recorder.saved?.first?.plan, .pro, "the add-time fallback")
    }

    /// An identity that cannot be established is an unusable session: a skip
    /// for that candidate, not a failure for the run.
    func testCandidateWhoseAccountCallFailsIsSkipped() async {
        let recorder = Recorder()
        let env = makeEnvironment(
            candidates: [makeCandidate(displayName: "Person 2", sessionKey: "sk", orgId: "org-1")],
            accounts: [],
            recorder: recorder,
            handler: { request in
                (HTTPURLResponse(url: request.url!, statusCode: 401,
                                 httpVersion: nil, headerFields: nil)!, Data())
            })

        let code = await SyncCommand.runAsync(env: env)

        XCTAssertEqual(code, 0)
        XCTAssertTrue(recorder.lines.contains("  Skipping Person 2 (session expired)"))
        XCTAssertEqual(recorder.saved, [], "nothing added")
    }
}

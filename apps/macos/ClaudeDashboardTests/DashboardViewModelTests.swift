import XCTest
@testable import ClaudeDashboard

@MainActor
final class DashboardViewModelTests: XCTestCase {

    private var tempDir: URL!
    private var defaultsSuiteName: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaultsSuiteName = "com.claude-dashboard.vm-tests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: defaultsSuiteName)!
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        defaults.removePersistentDomain(forName: defaultsSuiteName)
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    // MARK: - Helpers

    private func makeAccount(orgId: String? = nil, pinned: Bool = false, name: String = "Test", email: String? = nil, accountUuid: String? = nil, profilePath: String = "Profile 1") -> Account {
        Account(
            id: UUID(),
            name: name,
            email: email,
            chromeProfilePath: profilePath,
            chromeProfileName: nil,
            orgId: orgId,
            accountUuid: accountUuid,
            plan: .max5x,
            lastSynced: nil,
            status: .active,
            isPinned: pinned,
            source: .browser
        )
    }

    private func makeViewModel(detectorEmail: String? = nil) throws -> DashboardViewModel {
        let detectorFile = tempDir.appendingPathComponent(".claude.json-\(UUID().uuidString)")
        if let email = detectorEmail {
            let body = """
            {"oauthAccount":{"emailAddress":"\(email)"}}
            """
            try body.write(to: detectorFile, atomically: true, encoding: .utf8)
        }
        let detector = ClaudeCodeAccountDetector(fileURL: detectorFile)
        let store = AccountStore(defaults: defaults)
        return DashboardViewModel(accountStore: store, ccDetector: detector)
    }

    // MARK: - isActiveClaudeCodeAccount

    func testIsActiveClaudeCodeAccount_matchesByEmail() throws {
        let vm = try makeViewModel(detectorEmail: "user@example.com")
        let state = AccountUsageState(id: UUID(), account: makeAccount(email: "user@example.com"))
        XCTAssertTrue(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenEmailsDiffer() throws {
        let vm = try makeViewModel(detectorEmail: "user@example.com")
        let state = AccountUsageState(id: UUID(), account: makeAccount(email: "other@example.com"))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenDetectorReturnsNil() throws {
        let vm = try makeViewModel(detectorEmail: nil)
        let state = AccountUsageState(id: UUID(), account: makeAccount(email: "user@example.com"))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenAccountEmailNil() throws {
        let vm = try makeViewModel(detectorEmail: "user@example.com")
        let state = AccountUsageState(id: UUID(), account: makeAccount(email: nil))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }

    // MARK: - sortStates

    /// Builds a state whose burn rate resolves to `utilization / timeRemaining`.
    /// Higher burn rate sorts earlier under the existing logic.
    private func makeState(account: Account, utilization: Double, resetsIn: TimeInterval) -> AccountUsageState {
        let fiveHour = UsageLimit(
            utilization: utilization,
            resetsAt: Date().addingTimeInterval(resetsIn)
        )
        let sevenDay = UsageLimit(utilization: 0, resetsAt: nil)
        let usage = UsageData(
            fiveHour: fiveHour,
            sevenDay: sevenDay
        )
        return AccountUsageState(id: account.id, account: account, usage: usage)
    }

    func testSortStates_pinnedRespectedOverActiveCC() throws {
        let vm = try makeViewModel(detectorEmail: "active@x.com")
        // A is pinned but NOT the active CC account.
        // B matches the active CC account but is not pinned.
        // Expected order: A (pinned), then B (active).
        let a = makeAccount(pinned: true, name: "A", email: "other@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "active@x.com")
        vm.accountStates = [
            makeState(account: b, utilization: 10, resetsIn: 3600),
            makeState(account: a, utilization: 10, resetsIn: 3600),
        ]
        vm.sortStates()
        XCTAssertEqual(vm.accountStates.map(\.account.name), ["A", "B"])
    }

    func testSortStates_activeCCBoostedWhenNoPin() throws {
        let vm = try makeViewModel(detectorEmail: "active@x.com")
        // No pins. A has HIGHER burn rate than B. B matches active CC.
        // Expected order: B (active CC) first, A second.
        let a = makeAccount(pinned: false, name: "A", email: "other@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "active@x.com")
        vm.accountStates = [
            makeState(account: a, utilization: 90, resetsIn: 3600),  // high burn rate
            makeState(account: b, utilization: 10, resetsIn: 3600),  // low burn rate but active
        ]
        vm.sortStates()
        XCTAssertEqual(vm.accountStates.map(\.account.name), ["B", "A"])
    }

    func testSortStates_fallsBackToBurnRate_whenNoMatch() throws {
        let vm = try makeViewModel(detectorEmail: "nonexistent@x.com")
        // No pins. No account matches active CC email.
        // Expected: sorted by burn rate alone (A before B).
        let a = makeAccount(pinned: false, name: "A", email: "a@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "b@x.com")
        vm.accountStates = [
            makeState(account: b, utilization: 10, resetsIn: 3600),
            makeState(account: a, utilization: 90, resetsIn: 3600),
        ]
        vm.sortStates()
        XCTAssertEqual(vm.accountStates.map(\.account.name), ["A", "B"])
    }

    func testSortStates_fallsBackToBurnRate_whenDetectorHasNoEmail() throws {
        let vm = try makeViewModel(detectorEmail: nil)
        // Detector returned nil, so active-CC layer is inert.
        let a = makeAccount(pinned: false, name: "A", email: "a@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "b@x.com")
        vm.accountStates = [
            makeState(account: b, utilization: 10, resetsIn: 3600),
            makeState(account: a, utilization: 90, resetsIn: 3600),
        ]
        vm.sortStates()
        XCTAssertEqual(vm.accountStates.map(\.account.name), ["A", "B"])
    }

    // MARK: - menuBarLabel

    func testMenuBarLabel_showsPinnedAccount() throws {
        let vm = try makeViewModel(detectorEmail: nil)
        let a = makeAccount(pinned: true, name: "A")
        let b = makeAccount(pinned: false, name: "B")
        vm.accountStates = [
            makeState(account: a, utilization: 30, resetsIn: 3600),
            makeState(account: b, utilization: 90, resetsIn: 3600),
        ]
        vm.sortStates()
        // Menu bar should show pinned account A (30%), not B (90%)
        XCTAssertTrue(vm.menuBarPercentText.hasPrefix("30%"))
    }

    func testMenuBarLabel_showsFirstSortedAccount_whenNoPinned() throws {
        let vm = try makeViewModel(detectorEmail: "active@x.com")
        // B is active CC account with low utilization.
        // A has higher utilization but is not the active CC account.
        // After sort: B first (active CC), then A.
        // Menu bar should show B's data (first sorted), not A's (highest utilization).
        let a = makeAccount(pinned: false, name: "A", email: "other@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "active@x.com")
        vm.accountStates = [
            makeState(account: a, utilization: 90, resetsIn: 3600),
            makeState(account: b, utilization: 10, resetsIn: 3600),
        ]
        vm.sortStates()
        // Before fix: would show "90%" (max utilization = A)
        // After fix: shows "10%" (first sorted = B, the active CC account)
        XCTAssertTrue(vm.menuBarPercentText.hasPrefix("10%"))
    }

    // MARK: - shouldRunSavedCommand

    /// The command fires to (re)start a reset circle. The signal is `resetsAt == nil`
    /// (Claude returns `resets_at: null` after a window resets, until the next first
    /// request), NOT utilization — an idle window sits at 0% while its circle keeps
    /// running. Utilization is therefore fixed at 0 here to prove it's irrelevant.
    private func makeUsage(fiveHourReset: Date?, sevenDayReset: Date?) -> UsageData {
        UsageData(
            fiveHour: UsageLimit(utilization: 0, resetsAt: fiveHourReset),
            sevenDay: UsageLimit(utilization: 0, resetsAt: sevenDayReset)
        )
    }

    func testShouldRunSavedCommand_falseWhenUsageNil() {
        // Fetch failed → window state unknown → do not fire.
        XCTAssertFalse(DashboardViewModel.shouldRunSavedCommand(for: nil))
    }

    func testShouldRunSavedCommand_trueWhenFiveHourResetNil() {
        let future = Date().addingTimeInterval(3600)
        XCTAssertTrue(DashboardViewModel.shouldRunSavedCommand(
            for: makeUsage(fiveHourReset: nil, sevenDayReset: future)))
    }

    func testShouldRunSavedCommand_trueWhenSevenDayResetNil() {
        let future = Date().addingTimeInterval(3600)
        XCTAssertTrue(DashboardViewModel.shouldRunSavedCommand(
            for: makeUsage(fiveHourReset: future, sevenDayReset: nil)))
    }

    func testShouldRunSavedCommand_trueWhenBothResetNil() {
        XCTAssertTrue(DashboardViewModel.shouldRunSavedCommand(
            for: makeUsage(fiveHourReset: nil, sevenDayReset: nil)))
    }

    func testShouldRunSavedCommand_falseWhenBothResetPresent_evenAtZeroUtilization() {
        // The bug: an idle account (0% on both windows) whose circles are still
        // running must NOT fire. Only a nil reset (circle gone) should.
        let future = Date().addingTimeInterval(3600)
        XCTAssertFalse(DashboardViewModel.shouldRunSavedCommand(
            for: makeUsage(fiveHourReset: future, sevenDayReset: future)))
    }

    func testSortStates_activeCCNotBoosted_whenOtherAccountIsPinned() throws {
        let vm = try makeViewModel(detectorEmail: "active@x.com")
        // C is pinned. B is the active CC account but unpinned.
        // A is unpinned with a higher burn rate than B.
        // Expected: C first (pinned), then A and B by burn rate (CC boost is inactive
        // because some account is pinned).
        let a = makeAccount(pinned: false, name: "A", email: "a@x.com")
        let b = makeAccount(pinned: false, name: "B", email: "active@x.com")
        let c = makeAccount(pinned: true, name: "C", email: "c@x.com")
        vm.accountStates = [
            makeState(account: b, utilization: 10, resetsIn: 3600),
            makeState(account: a, utilization: 90, resetsIn: 3600),
            makeState(account: c, utilization: 50, resetsIn: 3600),
        ]
        vm.sortStates()
        XCTAssertEqual(vm.accountStates.map(\.account.name), ["C", "A", "B"])
    }

    // MARK: - Identity backfill

    /// A view model wired to an explicit store and a MockURLProtocol-backed
    /// API service, so a refresh can be driven end to end.
    /// `cookieProvider` is keyed by the account's profile path, so a multi-account
    /// test can hand one profile a working session and another none at all.
    private func makeViewModelWithStore(
        cookies: ChromeCookieResult = ChromeCookieResult(sessionKey: nil, orgId: nil),
        cookieProvider: ((String, Browser) -> ChromeCookieResult)? = nil
    ) throws -> (DashboardViewModel, AccountStore) {
        let detectorFile = tempDir.appendingPathComponent(".claude.json-\(UUID().uuidString)")
        let detector = ClaudeCodeAccountDetector(fileURL: detectorFile)
        let store = AccountStore(defaults: defaults)
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let api = UsageAPIService(session: URLSession(configuration: config))
        // Isolated SQLite files: a refresh this view model starts can outlive the test,
        // and the default stores point at the user's real databases.
        let vm = DashboardViewModel(
            accountStore: store,
            apiService: api,
            logStore: UsageLogStore(dbPath: tempDir.appendingPathComponent("usage.sqlite").path),
            commandLogStore: CommandLogStore(dbPath: tempDir.appendingPathComponent("commands.sqlite").path),
            ccDetector: detector,
            cookieProvider: cookieProvider ?? { _, _ in cookies })
        return (vm, store)
    }

    private static let emptyUsageJSON = """
    {"five_hour":{"utilization":0,"resets_at":null},"seven_day":{"utilization":0,"resets_at":null}}
    """

    private func respond(accountBody: String) {
        MockURLProtocol.requestHandler = { request in
            let path = request.url?.path ?? ""
            let body: String
            if path == "/api/account" {
                body = accountBody
            } else if path.hasSuffix("/usage") {
                body = Self.emptyUsageJSON
            } else {
                body = "[]"
            }
            let response = HTTPURLResponse(url: request.url!, statusCode: 200,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data(body.utf8))
        }
    }

    func testRefreshBackfillsAccountUuidAndEmailWhenMissing() async throws {
        let (vm, store) = try makeViewModelWithStore()
        // A stored account written before accountUuid existed.
        let account = makeAccount(orgId: "org-1", email: nil)
        store.addAccount(account)
        store.saveSessionKey("sk-test", for: account.id)

        respond(accountBody: #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[]}"#)

        // AccountStore.$accounts reaches the view model via .receive(on: .main),
        // which dispatches asynchronously; without yielding here, refreshAll's
        // synchronous account-collection loop would run against the stale
        // (pre-add) accountStates and never touch this account at all.
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 1, "sanity: account must reach accountStates before refreshAll runs")

        await vm.refreshAll()

        let updated = store.accounts.first { $0.id == account.id }
        XCTAssertEqual(updated?.accountUuid, "acct-1")
        XCTAssertEqual(updated?.email, "person@example.com")
    }

    // MARK: - Plan-tier refresh (contract/cases/plan-refresh.json)

    /// Responds to `/api/organizations` with `orgsBody`, or a 500 when it is nil.
    /// `/api/account` and `/usage` always succeed, so the refresh reaches the
    /// plan-update step either way.
    private func respondOrganizations(_ orgsBody: String?) {
        MockURLProtocol.requestHandler = { request in
            let path = request.url?.path ?? ""
            if path == "/api/organizations" {
                guard let orgsBody else {
                    let failure = HTTPURLResponse(url: request.url!, statusCode: 500,
                                                  httpVersion: nil, headerFields: nil)!
                    return (failure, Data())
                }
                let ok = HTTPURLResponse(url: request.url!, statusCode: 200,
                                         httpVersion: nil, headerFields: nil)!
                return (ok, Data(orgsBody.utf8))
            }
            let body = path == "/api/account"
                ? #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[]}"#
                : Self.emptyUsageJSON
            let ok = HTTPURLResponse(url: request.url!, statusCode: 200,
                                     httpVersion: nil, headerFields: nil)!
            return (ok, Data(body.utf8))
        }
    }

    /// The heal path: a tier that is wrong in the store is corrected by the
    /// next refresh, without deleting and re-adding the account.
    func testRefreshCorrectsAWrongStoredPlanTier() async throws {
        let (vm, store) = try makeViewModelWithStore()
        let account = makeAccount(orgId: "org-1", accountUuid: "acct-1")  // stored .max5x
        store.addAccount(account)
        store.saveSessionKey("sk-test", for: account.id)

        respondOrganizations(#"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_pro"]}]"#)

        // See testRefreshBackfillsAccountUuidAndEmailWhenMissing: the store's
        // publish reaches accountStates asynchronously.
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 1, "sanity: account must reach accountStates before refreshAll runs")

        await vm.refreshAll()

        XCTAssertEqual(store.accounts.first { $0.id == account.id }?.plan, .pro)
    }

    /// Rule 1: an unresolvable hint leaves the stored tier alone. A network
    /// blip must not overwrite a known-good plan.
    func testRefreshLeavesThePlanAloneWhenOrganizationsFails() async throws {
        let (vm, store) = try makeViewModelWithStore()
        let account = makeAccount(orgId: "org-1", accountUuid: "acct-1")  // stored .max5x
        store.addAccount(account)
        store.saveSessionKey("sk-test", for: account.id)

        respondOrganizations(nil)

        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 1, "sanity: account must reach accountStates before refreshAll runs")

        await vm.refreshAll()

        XCTAssertEqual(store.accounts.first { $0.id == account.id }?.plan, .max5x,
                       "a failed /api/organizations must not rewrite the tier")
    }

    func testRefreshDoesNotOverwriteAnExistingEmailOrOrgId() async throws {
        let (vm, store) = try makeViewModelWithStore()
        let account = makeAccount(orgId: "org-1", email: "kept@example.com")
        store.addAccount(account)
        store.saveSessionKey("sk-test", for: account.id)

        respond(accountBody: #"{"uuid":"acct-1","email_address":"other@example.com","memberships":[{"role":"user","organization":{"uuid":"org-other","name":"Other","capabilities":["chat"]}}]}"#)

        // See the matching comment in testRefreshBackfillsAccountUuidAndEmailWhenMissing:
        // the store's publish reaches accountStates asynchronously.
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 1, "sanity: account must reach accountStates before refreshAll runs")

        await vm.refreshAll()

        let updated = store.accounts.first { $0.id == account.id }
        XCTAssertEqual(updated?.accountUuid, "acct-1")
        XCTAssertEqual(updated?.email, "kept@example.com",
                       "backfill must not overwrite an existing email")
        XCTAssertEqual(updated?.orgId, "org-1",
                       "backfill must not re-resolve orgId")
    }

    // MARK: - Re-sync org resolution

    /// `/api/account` for an account that belongs to an api-only org and a chat org.
    private static func accountBody(uuid: String, email: String = "person@example.com") -> String {
        """
        {"uuid":"\(uuid)","email_address":"\(email)","memberships":[
          {"organization":{"uuid":"org-api","name":"API","capabilities":["api","api_individual"]}},
          {"organization":{"uuid":"org-good","name":"Example Co","capabilities":["chat"]}}
        ]}
        """
    }

    func testResyncResolvesOrgIdByRuleInsteadOfWritingTheCookieValue() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-new", orgId: "org-api"))
        let account = makeAccount(orgId: "org-good", email: "person@example.com", accountUuid: "acct-1")
        store.addAccount(account)

        respond(accountBody: Self.accountBody(uuid: "acct-1", email: "other@example.com"))
        await Task.yield()

        await vm.resyncAccount(account.id)

        XCTAssertEqual(store.accounts.first?.orgId, "org-good",
                       "the cookie's lastActiveOrg is an api-only org; resync must fall through to the chat org")
        XCTAssertEqual(store.accounts.first?.email, "person@example.com",
                       "backfill must never overwrite an email the record already has")
    }

    /// Regression guard: an unreachable `/api/account` must not cost the user the
    /// recovery path. `refreshAll` skips `.expired`, so resync has to save the key
    /// and mark the account active for a retry to happen at all.
    func testResyncKeepsTheStoredOrgIdWhenAccountFetchFails() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-new", orgId: "org-api"))
        var account = makeAccount(orgId: "org-good", email: "person@example.com", accountUuid: "acct-1")
        account.status = .expired
        store.addAccount(account)

        MockURLProtocol.requestHandler = { request in
            let path = request.url?.path ?? ""
            let status = path == "/api/account" ? 500 : 200
            let body = path.hasSuffix("/usage") ? Self.emptyUsageJSON : "[]"
            let response = HTTPURLResponse(url: request.url!, statusCode: status,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data(body.utf8))
        }
        await Task.yield()

        await vm.resyncAccount(account.id)

        let updated = store.accounts.first
        XCTAssertEqual(updated?.orgId, "org-good", "an unresolvable org must never overwrite the stored one")
        XCTAssertEqual(updated?.status, .active)
        XCTAssertEqual(store.loadSessionKey(for: account.id), "sk-new")
    }

    func testResyncReportsAnAccountWithNoChatOrgAndLeavesOrgIdAlone() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-new", orgId: "org-api"))
        let account = makeAccount(orgId: "org-good", email: "person@example.com", accountUuid: "acct-1")
        store.addAccount(account)

        respond(accountBody: #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[{"organization":{"uuid":"org-api","name":"API","capabilities":["api","api_individual"]}}]}"#)
        await Task.yield()

        await vm.resyncAccount(account.id)

        XCTAssertEqual(store.accounts.first?.orgId, "org-good")
        XCTAssertNotNil(vm.accountStates.first?.error,
                        "an account with no chat org must be reported, not silently left as-is")
    }

    func testResyncRefusesASessionBelongingToADifferentAccount() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-other", orgId: "org-good"))
        let account = makeAccount(orgId: "org-good", email: "person@example.com", accountUuid: "acct-1")
        store.addAccount(account)
        store.saveSessionKey("sk-mine", for: account.id)

        respond(accountBody: Self.accountBody(uuid: "acct-2", email: "other@example.com"))
        await Task.yield()

        await vm.resyncAccount(account.id)

        XCTAssertEqual(store.loadSessionKey(for: account.id), "sk-mine",
                       "another account's session key must never land on this record")
        XCTAssertNotNil(vm.accountStates.first?.error)
    }

    func testResyncBackfillsAccountUuidAndEmailOnALegacyRecord() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-new", orgId: "org-good"))
        let account = makeAccount(orgId: "org-good", email: nil, accountUuid: nil)
        store.addAccount(account)

        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        await vm.resyncAccount(account.id)

        let updated = store.accounts.first
        XCTAssertEqual(updated?.accountUuid, "acct-1")
        XCTAssertEqual(updated?.email, "person@example.com")
    }

    /// A pasted-key account has no browser profile. Without a gate, re-sync falls
    /// through to the cookie provider with an empty profile path and tells the user
    /// to sign in to a profile that never existed — about the one account type a
    /// cookie re-read cannot help.
    func testResyncReportsAPastedKeyAccountInsteadOfReadingAProfileThatDoesNotExist() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-from-a-browser", orgId: "org-good"))
        var account = makeAccount(orgId: "org-good", email: "person@example.com",
                                  accountUuid: "acct-1")
        account.source = .manual
        account.chromeProfilePath = ""
        store.addAccount(account)
        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        await vm.resyncAccount(account.id)

        XCTAssertNil(store.loadSessionKey(for: account.id),
                     "a browser profile's session key must never land on a pasted-key account")
        let error = try XCTUnwrap(vm.accountStates.first?.error,
                                  "the card must say why re-sync cannot help this account")
        XCTAssertTrue(error.lowercased().contains("paste"),
                      "the message must name what actually fixes it; got: \(error)")
        XCTAssertFalse(error.contains("\"\""),
                       "the old message named an empty profile; got: \(error)")
    }

    // MARK: - Re-sync All

    /// One mock for a two-account fleet: `/api/account` answers per session key, so
    /// each browser profile resolves to its own identity, and every path is tallied.
    private func respondPerSessionKey(_ counter: RequestCounter,
                                      uuidForCookie: @escaping (String) -> String) {
        MockURLProtocol.requestHandler = { request in
            let path = request.url?.path ?? ""
            counter.record(path)
            let cookie = request.value(forHTTPHeaderField: "Cookie") ?? ""
            let body: String
            if path == "/api/account" {
                body = Self.accountBody(uuid: uuidForCookie(cookie))
            } else if path.hasSuffix("/usage") {
                body = Self.emptyUsageJSON
            } else {
                body = "[]"
            }
            let response = HTTPURLResponse(url: request.url!, statusCode: 200,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data(body.utf8))
        }
    }

    /// Regression guard for the "Re-sync All" loop: it used to await `resyncAccount`
    /// per account, and each of those spawned a whole-fleet `refreshAll`, so N
    /// accounts cost N refresh passes (N x N usage fetches). One scoped pass is enough.
    func testResyncAllFetchesUsageOncePerAccount() async throws {
        let counter = RequestCounter()
        let (vm, store) = try makeViewModelWithStore(cookieProvider: { path, _ in
            ChromeCookieResult(sessionKey: path == "Profile 1" ? "sk-1" : "sk-2", orgId: "org-good")
        })
        // The view model starts a whole-fleet refresh in `init`. Let it finish here,
        // against an empty store, so it cannot land in the middle of what this test
        // measures.
        try await Task.sleep(nanoseconds: 100_000_000)
        store.addAccount(makeAccount(orgId: "org-good", email: "a@example.com",
                                     accountUuid: "acct-1", profilePath: "Profile 1"))
        store.addAccount(makeAccount(orgId: "org-good", email: "b@example.com",
                                     accountUuid: "acct-2", profilePath: "Profile 2"))
        respondPerSessionKey(counter) { $0.contains("sk-1") ? "acct-1" : "acct-2" }
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 2, "sanity: both accounts must reach accountStates")

        await vm.resyncAll()
        // A refresh pass left running by a fire-and-forget `Task {}` lands here.
        try await Task.sleep(nanoseconds: 300_000_000)

        XCTAssertEqual(counter.count(pathSuffix: "/usage"), 2,
                       "one usage fetch per account; more means the loop is spawning extra refresh passes")
        XCTAssertEqual(counter.count(path: "/api/account"), 2,
                       "identity is read once per re-synced account")
    }

    /// Re-sync exists mostly for an expired account, so the pass that follows it must
    /// actually fetch that account. It sees `accountStates`, which the store refreshes
    /// asynchronously, and a stale `.expired` status there would silently skip the card.
    func testResyncRefreshesAnAccountThatWasExpired() async throws {
        let counter = RequestCounter()
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: "sk-new", orgId: "org-good"))
        // The view model starts a whole-fleet refresh in `init`. Let it finish here,
        // against an empty store, so it cannot land in the middle of what this test
        // measures.
        try await Task.sleep(nanoseconds: 100_000_000)
        var account = makeAccount(orgId: "org-good", email: "person@example.com", accountUuid: "acct-1")
        account.status = .expired
        store.addAccount(account)
        respondPerSessionKey(counter) { _ in "acct-1" }
        await Task.yield()

        await vm.resyncAccount(account.id)
        try await Task.sleep(nanoseconds: 300_000_000)

        XCTAssertEqual(counter.count(pathSuffix: "/usage"), 1,
                       "the re-synced account must be refreshed, not skipped as still-expired")
        XCTAssertNotNil(vm.accountStates.first?.usage, "the card must show the usage it just fetched")
    }

    /// Double-clicking "Re-sync All" must not spawn a second overlapping pass:
    /// the second call has to bail out while the first is still running.
    func testResyncAllIgnoresAReentrantCallWhileRunning() async throws {
        let counter = RequestCounter()
        let (vm, store) = try makeViewModelWithStore(cookieProvider: { path, _ in
            ChromeCookieResult(sessionKey: path == "Profile 1" ? "sk-1" : "sk-2", orgId: "org-good")
        })
        // The view model starts a whole-fleet refresh in `init`. Let it finish here,
        // against an empty store, so it cannot land in the middle of what this test
        // measures.
        try await Task.sleep(nanoseconds: 100_000_000)
        store.addAccount(makeAccount(orgId: "org-good", email: "a@example.com",
                                     accountUuid: "acct-1", profilePath: "Profile 1"))
        store.addAccount(makeAccount(orgId: "org-good", email: "b@example.com",
                                     accountUuid: "acct-2", profilePath: "Profile 2"))
        respondPerSessionKey(counter) { $0.contains("sk-1") ? "acct-1" : "acct-2" }
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 2, "sanity: both accounts must reach accountStates")

        // Both start on the MainActor; the second reaches the guard while the first
        // is suspended on its own awaits, because progress is set before any await.
        async let first: Void = vm.resyncAll()
        async let second: Void = vm.resyncAll()
        _ = await (first, second)
        try await Task.sleep(nanoseconds: 300_000_000)

        XCTAssertEqual(counter.count(pathSuffix: "/usage"), 2,
                       "a re-entrant Re-sync All must be ignored, not run a second pass")
    }

    /// The settings button binds to `resyncAllProgress`: non-nil (with a running
    /// count) while the pass runs, nil again when it ends — success or not.
    func testResyncAllPublishesProgressAndClearsItWhenDone() async throws {
        let counter = RequestCounter()
        let (vm, store) = try makeViewModelWithStore(cookieProvider: { path, _ in
            ChromeCookieResult(sessionKey: path == "Profile 1" ? "sk-1" : "sk-2", orgId: "org-good")
        })
        try await Task.sleep(nanoseconds: 100_000_000)
        store.addAccount(makeAccount(orgId: "org-good", email: "a@example.com",
                                     accountUuid: "acct-1", profilePath: "Profile 1"))
        store.addAccount(makeAccount(orgId: "org-good", email: "b@example.com",
                                     accountUuid: "acct-2", profilePath: "Profile 2"))
        respondPerSessionKey(counter) { $0.contains("sk-1") ? "acct-1" : "acct-2" }
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 2, "sanity: both accounts must reach accountStates")

        var seen: [(done: Int, total: Int)?] = []
        let cancellable = vm.$resyncAllProgress.sink { seen.append($0) }
        defer { cancellable.cancel() }

        await vm.resyncAll()
        try await Task.sleep(nanoseconds: 300_000_000)

        XCTAssertNil(vm.resyncAllProgress, "progress must clear when the pass ends")
        XCTAssertTrue(seen.contains { $0?.total == 2 },
                      "progress must have been published with total = 2 during the run")
        XCTAssertTrue(seen.contains { $0?.done == 2 },
                      "done must reach the account count before clearing")
    }

    /// A re-sync failure message must survive the refresh that follows another
    /// account's successful re-sync.
    func testResyncAllKeepsTheFailureMessageOfAnAccountWithNoCookies() async throws {
        let counter = RequestCounter()
        let broken = makeAccount(orgId: "org-good", email: "a@example.com",
                                 accountUuid: "acct-1", profilePath: "Profile 1")
        let healthy = makeAccount(orgId: "org-good", email: "b@example.com",
                                  accountUuid: "acct-2", profilePath: "Profile 2")
        let (vm, store) = try makeViewModelWithStore(cookieProvider: { path, _ in
            path == "Profile 1"
                ? ChromeCookieResult(sessionKey: nil, orgId: nil)
                : ChromeCookieResult(sessionKey: "sk-2", orgId: "org-good")
        })
        // The view model starts a whole-fleet refresh in `init`. Let it finish here,
        // against an empty store, so it cannot land in the middle of what this test
        // measures.
        try await Task.sleep(nanoseconds: 100_000_000)
        store.addAccount(broken)
        store.addAccount(healthy)
        // The broken account stays `.active` and keeps a stored key, so a
        // whole-fleet refresh would fetch it happily and clear the error.
        store.saveSessionKey("sk-1-stale", for: broken.id)
        respondPerSessionKey(counter) { _ in "acct-2" }
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 2, "sanity: both accounts must reach accountStates")

        await vm.resyncAll()
        try await Task.sleep(nanoseconds: 300_000_000)

        let state = vm.accountStates.first { $0.id == broken.id }
        XCTAssertNotNil(state?.error, "a re-sync failure must outlive the other account's refresh")
        XCTAssertTrue(state?.error?.contains("Re-sync failed") ?? false,
                      "expected the re-sync failure message, got: \(state?.error ?? "nil")")
    }

    // MARK: - Manual session key

    func testManualKeyAddsAnAccountWithNoProfileAndTheFirstChatOrg() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        let outcome = await vm.applyManualKey("  sk-pasted\n")

        XCTAssertEqual(outcome, .added(name: "person@example.com"))
        let added = try XCTUnwrap(store.accounts.first)
        XCTAssertEqual(added.source, .manual)
        XCTAssertEqual(added.chromeProfilePath, "")
        XCTAssertNil(added.chromeProfileName)
        XCTAssertEqual(added.orgId, "org-good", "no lastActiveOrg, so rule 2: the first chat org")
        XCTAssertEqual(added.accountUuid, "acct-1")
        XCTAssertEqual(store.loadSessionKey(for: added.id), "sk-pasted",
                       "the key is trimmed and stored encrypted")
    }

    func testManualKeyRepairsAStoredAccountWithoutTouchingItsOrgId() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        var account = makeAccount(orgId: "org-cookie", email: "person@example.com",
                                  accountUuid: "acct-1")
        account.status = .expired
        store.addAccount(account)
        // `org-cookie` is not in the memberships this body returns; a re-resolve
        // would rewrite orgId to `org-good`, which is exactly what must not happen.
        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        let outcome = await vm.applyManualKey("sk-pasted")

        XCTAssertEqual(outcome, .updated(name: "person@example.com"))
        let updated = try XCTUnwrap(store.accounts.first)
        XCTAssertEqual(updated.orgId, "org-cookie",
                       "a pasted key carries no org preference and must not demote a resolved orgId")
        XCTAssertEqual(updated.status, .active)
        XCTAssertEqual(updated.source, .browser, "a key never changes which source a record has")
        XCTAssertEqual(store.loadSessionKey(for: account.id), "sk-pasted")
        XCTAssertEqual(store.accounts.count, 1, "a repair must not add a second record")
    }

    func testManualKeyFillsAStoredOrgIdThatWasNil() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        var account = makeAccount(orgId: nil, email: "person@example.com", accountUuid: "acct-1")
        account.status = .expired
        store.addAccount(account)
        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        _ = await vm.applyManualKey("sk-pasted")

        XCTAssertEqual(store.accounts.first?.orgId, "org-good",
                       "there is nothing to demote, so a nil orgId is backfilled")
    }

    func testManualKeyRejectsAnAccountWithNoChatOrg() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        respond(accountBody: #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[{"organization":{"uuid":"org-api","name":"API","capabilities":["api","api_individual"]}}]}"#)
        await Task.yield()

        let outcome = await vm.applyManualKey("sk-pasted")

        XCTAssertEqual(outcome, .rejectedNoChatOrg)
        XCTAssertTrue(store.accounts.isEmpty, "an unresolvable org is never persisted as working")
    }

    /// The warning follows the resolve result, not the stored `orgId`. An
    /// account that kept a working-looking `orgId` but lost chat access polls a
    /// dead org forever, and a test of the resulting record stays silent on it.
    func testManualKeyWarnsWhenAStoredOrgIdSurvivesButNoChatOrgIsLeft() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        var account = makeAccount(orgId: "org-good", email: "person@example.com",
                                  accountUuid: "acct-1")
        account.status = .expired
        store.addAccount(account)
        respond(accountBody: #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[{"organization":{"uuid":"org-api","name":"API","capabilities":["api","api_individual"]}}]}"#)
        await Task.yield()

        let outcome = await vm.applyManualKey("sk-pasted")

        XCTAssertEqual(outcome, .updatedWithNoChatOrg(name: "person@example.com"),
                       "a repair with no chat org is reported, not silently accepted")
        let updated = try XCTUnwrap(store.accounts.first)
        XCTAssertEqual(updated.orgId, "org-good", "the stored orgId is kept and reported, not blanked")
        XCTAssertEqual(updated.status, .active, "the key is still worth saving")
    }

    func testManualKeyReportsAnUnacceptedKey() async throws {
        let (vm, store) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(url: request.url!, statusCode: 401,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data("{}".utf8))
        }
        await Task.yield()

        let outcome = await vm.applyManualKey("sk-dead")

        XCTAssertEqual(outcome, .keyNotAccepted)
        XCTAssertTrue(store.accounts.isEmpty)
    }

    func testManualKeyRejectsWhitespaceOnly() async throws {
        let (vm, _) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))

        let outcome = await vm.applyManualKey("   \n  ")

        XCTAssertEqual(outcome, .emptyKey, "no network call is worth making for this")
    }

    /// A sentinel that could not occur by accident, driven through the real
    /// `applyManualKey` path so the assertion can actually fail: a mapping that
    /// interpolated a key into its message would show this string verbatim.
    private static let leakSentinel = "sk-ant-sid01-LEAK-SENTINEL-9f3c"

    func testManualKeyMessageNeverLeaksTheKeyOnRejection() async throws {
        let (vm, _) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(url: request.url!, statusCode: 401,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data("{}".utf8))
        }
        await Task.yield()

        let outcome = await vm.applyManualKey(Self.leakSentinel)

        XCTAssertEqual(outcome, .keyNotAccepted)
        XCTAssertFalse(outcome.message.contains(Self.leakSentinel),
                       "a session key must never reach a user-visible string")
    }

    func testManualKeyMessageNeverLeaksTheKeyOnSuccess() async throws {
        let (vm, _) = try makeViewModelWithStore(
            cookies: ChromeCookieResult(sessionKey: nil, orgId: nil))
        respond(accountBody: Self.accountBody(uuid: "acct-1"))
        await Task.yield()

        let outcome = await vm.applyManualKey(Self.leakSentinel)

        XCTAssertEqual(outcome, .added(name: "person@example.com"))
        XCTAssertFalse(outcome.message.contains(Self.leakSentinel),
                       "a session key must never reach a user-visible string")
    }
}

/// Thread-safe tally of the paths `MockURLProtocol` served: the handler runs on
/// URLSession's threads and a refresh fires several requests at once.
final class RequestCounter {
    private let lock = NSLock()
    private var paths: [String] = []

    func record(_ path: String) {
        lock.lock()
        defer { lock.unlock() }
        paths.append(path)
    }

    func count(path: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return paths.filter { $0 == path }.count
    }

    func count(pathSuffix: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return paths.filter { $0.hasSuffix(pathSuffix) }.count
    }
}

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
}

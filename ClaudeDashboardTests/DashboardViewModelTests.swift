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
        super.tearDown()
    }

    // MARK: - Helpers

    private func makeAccount(orgId: String? = nil, pinned: Bool = false, name: String = "Test", email: String? = nil) -> Account {
        Account(
            id: UUID(),
            name: name,
            email: email,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: orgId,
            plan: .max5x,
            lastSynced: nil,
            status: .active,
            isPinned: pinned
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
            sevenDay: sevenDay,
            sevenDaySonnet: nil
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
}

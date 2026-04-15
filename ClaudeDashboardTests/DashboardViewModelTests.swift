import XCTest
@testable import ClaudeDashboard

@MainActor
final class DashboardViewModelTests: XCTestCase {

    private var tempDir: URL!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaults = UserDefaults(suiteName: "com.claude-dashboard.vm-tests-\(UUID().uuidString)")!
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        defaults.removePersistentDomain(forName: defaults.dictionaryRepresentation().keys.first ?? "")
        super.tearDown()
    }

    // MARK: - Helpers

    private func makeAccount(orgId: String?, pinned: Bool = false, name: String = "Test") -> Account {
        Account(
            id: UUID(),
            name: name,
            email: nil,
            chromeProfilePath: "Profile 1",
            chromeProfileName: nil,
            orgId: orgId,
            plan: .max5x,
            lastSynced: nil,
            status: .active,
            isPinned: pinned
        )
    }

    private func makeViewModel(detectorOrgId: String? = nil) -> DashboardViewModel {
        let detectorFile = tempDir.appendingPathComponent(".claude.json-\(UUID().uuidString)")
        if let orgId = detectorOrgId {
            let body = """
            {"oauthAccount":{"organizationUuid":"\(orgId)"}}
            """
            try? body.write(to: detectorFile, atomically: true, encoding: .utf8)
        }
        let detector = ClaudeCodeAccountDetector(fileURL: detectorFile)
        let store = AccountStore(defaults: defaults)
        return DashboardViewModel(accountStore: store, ccDetector: detector)
    }

    // MARK: - isActiveClaudeCodeAccount

    func testIsActiveClaudeCodeAccount_matchesByOrgId() {
        let vm = makeViewModel(detectorOrgId: "org-abc")
        let state = AccountUsageState(id: UUID(), account: makeAccount(orgId: "org-abc"))
        XCTAssertTrue(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenOrgIdsDiffer() {
        let vm = makeViewModel(detectorOrgId: "org-abc")
        let state = AccountUsageState(id: UUID(), account: makeAccount(orgId: "org-xyz"))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenDetectorReturnsNil() {
        let vm = makeViewModel(detectorOrgId: nil)
        let state = AccountUsageState(id: UUID(), account: makeAccount(orgId: "org-abc"))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }

    func testIsActiveClaudeCodeAccount_falseWhenAccountOrgIdNil() {
        let vm = makeViewModel(detectorOrgId: "org-abc")
        let state = AccountUsageState(id: UUID(), account: makeAccount(orgId: nil))
        XCTAssertFalse(vm.isActiveClaudeCodeAccount(state))
    }
}

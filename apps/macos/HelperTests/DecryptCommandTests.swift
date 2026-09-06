import XCTest

/// `decrypt` driven as a real process. It never touches the network; what it
/// does touch is the account store and CryptoService, so every case here runs
/// against a throwaway UserDefaults suite.
/// `contract/helper-cli.md` "decrypt" is the source of truth.
final class DecryptCommandTests: XCTestCase {

    private var suite: String!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        super.tearDown()
    }

    private func runDecrypt() -> HelperProcess.Result {
        HelperProcess.run(["decrypt"], env: [HelperAccountStore.suiteVariable: suite])
    }

    func testNoStoredAccountsAtAll() {
        let result = runDecrypt()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "No accounts found. Run: claude-dashboard-cli sync\n")
    }

    /// The gate is `status == .active && orgId != nil`, even though the message
    /// names session keys — the wording is a known misnomer the contract pins.
    func testStoredButNothingPassesTheInclusionFilter() {
        StoreFixture.seed([
            StoreFixture.account(name: "expired@example.com", orgId: "org-1", status: .expired),
            StoreFixture.account(name: "no-org@example.com", orgId: nil)
        ], intoSuite: suite)

        let result = runDecrypt()

        XCTAssertEqual(result.exitCode, 1)
        XCTAssertEqual(result.stderr, "No active accounts with session keys found.\n")
    }

    func testIncludedAccountIsProjectedWithSortedKeys() throws {
        StoreFixture.seed([
            StoreFixture.account(name: "me@example.com", orgId: "org-1", plan: .max20x),
            StoreFixture.account(name: "old@example.com", orgId: "org-2", status: .expired)
        ], intoSuite: suite)

        let result = runDecrypt()
        let out = result.stdoutText

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertFalse(out.contains("org-2"), "an expired account must be excluded")

        // Alphabetical key order is contract, not struct-declaration order.
        // XCTUnwrap rather than `!`: a key genuinely missing from `out` (e.g. a
        // mutation that also drops `sessionKey` when decrypt fails) must fail
        // this one assertion, not crash the test runner and silently drop the
        // rest of the suite.
        let order = try ["email", "name", "orgId", "plan", "sessionKey", "status"]
            .map { key in try XCTUnwrap(out.range(of: "\"\(key)\""), "missing key \(key) in: \(out)").lowerBound }
        XCTAssertEqual(order, order.sorted(), "keys must be alphabetical: \(out)")

        XCTAssertTrue(out.contains("\"plan\" : \"Max 20x\""), out)
    }

    /// A stored value that is not a valid ciphertext comes back verbatim: the
    /// command falls back to the stored string with no error path, so the
    /// caller receives ciphertext with no signal that decryption failed.
    func testUndecryptableSessionKeyIsPassedThroughVerbatim() {
        StoreFixture.seed([
            StoreFixture.account(name: "me@example.com", orgId: "org-1",
                                 sessionKey: "NOT-A-CIPHERTEXT")
        ], intoSuite: suite)

        let result = runDecrypt()

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertTrue(result.stdoutText.contains("\"sessionKey\" : \"NOT-A-CIPHERTEXT\""),
                      result.stdoutText)
    }

    /// A six-field projection means all six keys, always — a nil `email` or
    /// `sessionKey` must still appear as JSON `null`, not vanish. Built via
    /// `Account`'s own initializer rather than `StoreFixture.account`: that
    /// builder's `email` parameter always falls back to `name` when omitted,
    /// so it cannot express a genuinely nil email.
    func testNilEmailAndSessionKeyStillProjectAllSixKeysAsNull() {
        let account = Account(
            id: UUID(),
            name: "me@example.com",
            email: nil,
            chromeProfilePath: "Default",
            chromeProfileName: nil,
            orgId: "org-1",
            accountUuid: "acct-fixture",
            sessionKey: nil,
            browser: .chrome,
            plan: .pro,
            lastSynced: Date(timeIntervalSince1970: 1_000_000),
            status: .active,
            source: .browser
        )
        StoreFixture.seed([account], intoSuite: suite)

        let result = runDecrypt()
        let out = result.stdoutText

        XCTAssertEqual(result.exitCode, 0)
        for key in ["email", "name", "orgId", "plan", "sessionKey", "status"] {
            XCTAssertTrue(out.contains("\"\(key)\""), "missing key \(key) in: \(out)")
        }
        XCTAssertTrue(out.contains("\"email\" : null"), out)
        XCTAssertTrue(out.contains("\"sessionKey\" : null"), out)
    }

    /// Cross-process agreement on the HKDF-over-IOPlatformUUID key: this
    /// process encrypts, the child decrypts. The precondition matters — if
    /// IOPlatformUUID were unreadable here, encrypt would return nil, the
    /// fixture would hold plaintext, and the test would pass proving nothing.
    func testKeyEncryptedHereIsDecryptedByTheChildProcess() throws {
        let plaintext = "sk-fake-round-trip-key"
        let ciphertext = try XCTUnwrap(CryptoService.encrypt(plaintext),
                                       "CryptoService could not encrypt in the test process")
        XCTAssertNotEqual(ciphertext, plaintext)

        StoreFixture.seed([
            StoreFixture.account(name: "me@example.com", orgId: "org-1", sessionKey: ciphertext)
        ], intoSuite: suite)

        let result = runDecrypt()

        XCTAssertEqual(result.exitCode, 0)
        XCTAssertTrue(result.stdoutText.contains("\"sessionKey\" : \"\(plaintext)\""),
                      result.stdoutText)
    }
}

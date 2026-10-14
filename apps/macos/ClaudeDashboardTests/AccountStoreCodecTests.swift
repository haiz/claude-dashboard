import XCTest
@testable import ClaudeDashboard

/// `contract/account-schema.md` "A record this build cannot read": one record a
/// newer build wrote (say, a browser this build has no case for) costs only that
/// record, and is written back untouched.
final class AccountStoreCodecTests: XCTestCase {

    private func account(name: String, lastSynced: Date? = nil) -> Account {
        Account(
            id: UUID(),
            name: name,
            email: nil,
            chromeProfilePath: "Default",
            chromeProfileName: nil,
            orgId: "org-1",
            plan: .pro,
            lastSynced: lastSynced,
            status: .active,
            source: .browser
        )
    }

    /// A record as a newer build would write it: valid JSON, but a `browser`
    /// value this build has no case for.
    private func newerRecord() throws -> [String: Any] {
        let data = try JSONEncoder().encode(account(name: "from-a-newer-build"))
        var object = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        object["browser"] = "someFutureBrowser"
        return object
    }

    private func storeData(_ records: [Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: records)
    }

    func testDecodeKeepsReadableRecordsAndSetsTheRestAside() throws {
        let known = account(name: "known")
        let knownObject = try JSONSerialization.jsonObject(with: JSONEncoder().encode(known))
        let data = try storeData([knownObject, try newerRecord()])

        let decoded = try XCTUnwrap(AccountStoreCodec.decode(data))

        XCTAssertEqual(decoded.accounts, [known])
        XCTAssertEqual(decoded.unreadable.count, 1)
    }

    func testDecodeIsNilWhenTheStoreIsNotAJSONArray() {
        XCTAssertNil(AccountStoreCodec.decode(Data(#"[{"id":"x","name":"tru"#.utf8)))
        XCTAssertNil(AccountStoreCodec.decode(Data("{}".utf8)))
    }

    func testANonObjectElementIsKeptRatherThanCrashing() throws {
        let decoded = try XCTUnwrap(AccountStoreCodec.decode(Data("[42]".utf8)))

        XCTAssertEqual(decoded.accounts, [])
        XCTAssertEqual(decoded.unreadable.count, 1)
    }

    func testEncodeWritesTheUnreadableRecordsOfTheExistingStoreBackUnchanged() throws {
        let newer = try newerRecord()
        let existing = try storeData([newer])
        let known = account(name: "known")

        let written = try AccountStoreCodec.encode([known], over: existing)

        let records = try XCTUnwrap(try JSONSerialization.jsonObject(with: written) as? [Any])
        XCTAssertEqual(records.count, 2)
        XCTAssertEqual(records.last as? NSDictionary, newer as NSDictionary)
        XCTAssertEqual(AccountStoreCodec.decode(written)?.accounts, [known])
    }

    /// A fractional `lastSynced` is the value most likely to drift if the
    /// readable records were re-serialized carelessly next to the kept ones.
    func testReadableRecordsRoundTripExactlyNextToKeptOnes() throws {
        let known = account(name: "known", lastSynced: Date(timeIntervalSinceReferenceDate: 813_456_789.123_456_7))
        let existing = try storeData([try newerRecord()])

        let written = try AccountStoreCodec.encode([known], over: existing)

        XCTAssertEqual(AccountStoreCodec.decode(written)?.accounts, [known])
    }

    func testEncodeWithNothingToKeepIsPlainJSONEncoderOutput() throws {
        let known = account(name: "known", lastSynced: Date(timeIntervalSinceReferenceDate: 1.5))

        XCTAssertEqual(try AccountStoreCodec.encode([known], over: nil), try JSONEncoder().encode([known]))
    }
}

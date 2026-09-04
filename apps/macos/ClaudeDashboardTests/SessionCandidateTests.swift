import XCTest
@testable import ClaudeDashboard

/// `validate` answers a question and writes nothing. Its callers have three
/// different behaviours on a duplicate, so a shared function that wrote would
/// need a mode flag.
final class SessionCandidateTests: XCTestCase {

    private static let accountBody = """
    {"uuid":"acct-1","email_address":"person@example.com","memberships":[
      {"organization":{"uuid":"org-good","name":"Example Co","capabilities":["chat"]}}
    ]}
    """

    private func makeService() -> UsageAPIService {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        return UsageAPIService(session: URLSession(configuration: config))
    }

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    private func respond(accountStatus: Int = 200, orgStatus: Int = 200) {
        MockURLProtocol.requestHandler = { request in
            let path = request.url?.path ?? ""
            let isAccount = path == "/api/account"
            let isOrgs = path == "/api/organizations"
            let status = isAccount ? accountStatus : (isOrgs ? orgStatus : 200)
            let body = isAccount ? Self.accountBody : "[]"
            let response = HTTPURLResponse(url: request.url!, statusCode: status,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data(body.utf8))
        }
    }

    func testReturnsNilWhenTheSessionIsNotAccepted() async {
        respond(accountStatus: 401)

        let result = await SessionCandidate.validate(
            sessionKey: "sk-dead", against: [], apiService: makeService())

        XCTAssertNil(result, "an unusable session has no identity to report")
    }

    func testReportsNoDuplicateAgainstAnEmptyStore() async {
        respond()

        let result = await SessionCandidate.validate(
            sessionKey: "sk-live", against: [], apiService: makeService())

        XCTAssertEqual(result?.identity.uuid, "acct-1")
        XCTAssertNil(result?.duplicateIndex)
        XCTAssertNotNil(result?.orgs, "the orgs call succeeded, so this should not be nil")
    }

    func testOrgsFailureDoesNotInvalidateAnOtherwiseGoodSession() async {
        respond(orgStatus: 500)

        let result = await SessionCandidate.validate(
            sessionKey: "sk-live", against: [], apiService: makeService())

        XCTAssertEqual(result?.identity.uuid, "acct-1",
                       "session validity is established by /api/account, not this call")
        XCTAssertNil(result?.orgs, "a failed orgs fetch reports as absent, not fatal")
    }

    func testReportsTheIndexOfTheMatchingStoredRecord() async {
        respond()
        let stored = [
            StoredIdentity(accountUuid: "acct-other", email: "other@example.com"),
            StoredIdentity(accountUuid: "acct-1", email: "person@example.com")
        ]

        let result = await SessionCandidate.validate(
            sessionKey: "sk-live", against: stored, apiService: makeService())

        XCTAssertEqual(result?.duplicateIndex, 1,
                       "the caller needs which record matched, not merely that one did")
    }
}

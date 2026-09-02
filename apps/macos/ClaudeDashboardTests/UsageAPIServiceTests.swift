import XCTest
@testable import ClaudeDashboard

final class UsageAPIServiceTests: XCTestCase {

    override func setUp() {
        super.setUp()
        URLProtocol.registerClass(MockURLProtocol.self)
    }

    override func tearDown() {
        URLProtocol.unregisterClass(MockURLProtocol.self)
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    func testFetchUsageSuccess() async throws {
        let responseJSON = """
        {
          "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" }
        }
        """.data(using: .utf8)!

        MockURLProtocol.requestHandler = { request in
            XCTAssertEqual(request.url?.path, "/api/organizations/org-123/usage")
            XCTAssertTrue(request.value(forHTTPHeaderField: "Cookie")?.contains("sessionKey=sk-test") ?? false)

            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil
            )!
            return (response, responseJSON)
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)

        let service = UsageAPIService(session: session)
        let result = try await service.fetchUsage(orgId: "org-123", sessionKey: "sk-test")

        XCTAssertEqual(result.usage.fiveHour.utilization, 42.0)
        XCTAssertEqual(result.usage.sevenDay.utilization, 18.0)
        XCTAssertNil(result.newSessionKey)
    }

    func testFetchUsageSessionRefresh() async throws {
        let responseJSON = """
        {
          "five_hour": { "utilization": 10.0, "resets_at": null },
          "seven_day": { "utilization": 5.0, "resets_at": null }
        }
        """.data(using: .utf8)!

        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil,
                headerFields: ["Set-Cookie": "sessionKey=sk-new-key; Path=/; HttpOnly"]
            )!
            return (response, responseJSON)
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)

        let service = UsageAPIService(session: session)
        let result = try await service.fetchUsage(orgId: "org-123", sessionKey: "sk-old")

        XCTAssertEqual(result.newSessionKey, "sk-new-key")
    }

    func testFetchUsageAuthError() async throws {
        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 403,
                httpVersion: nil, headerFields: nil
            )!
            return (response, Data())
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)

        let service = UsageAPIService(session: session)

        do {
            _ = try await service.fetchUsage(orgId: "org-123", sessionKey: "sk-expired")
            XCTFail("Should have thrown")
        } catch UsageAPIError.authExpired {
            // expected
        }
    }

    func testFetchFullUsageReturnsUsage() async throws {
        // The usage endpoint no longer carries a plan signal; it just returns usage.
        let responseJSON = """
        {
          "five_hour": { "utilization": 30.0, "resets_at": null },
          "seven_day": { "utilization": 10.0, "resets_at": null },
          "extra_usage": { "is_enabled": false, "disabled_reason": "out_of_credits" }
        }
        """.data(using: .utf8)!

        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil
            )!
            return (response, responseJSON)
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)

        let service = UsageAPIService(session: session)
        let (usage, _) = try await service.fetchFullUsage(orgId: "org-123", sessionKey: "sk-test")

        XCTAssertEqual(usage.fiveHour.utilization, 30.0)
        XCTAssertEqual(usage.sevenDay.utilization, 10.0)
    }

    // Plan tier is derived solely from the organizations endpoint's capabilities.
    private func planHint(forCapabilities caps: [String]) async throws -> AccountPlan? {
        let capsJSON = caps.map { "\"\($0)\"" }.joined(separator: ", ")
        let responseJSON = """
        [ { "uuid": "org-123", "name": "test@example.com's Organization", "capabilities": [\(capsJSON)] } ]
        """.data(using: .utf8)!

        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil
            )!
            return (response, responseJSON)
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let session = URLSession(configuration: config)

        let service = UsageAPIService(session: session)
        let orgs = try await service.fetchOrganizations(sessionKey: "sk-test")
        return orgs.first?.planHint
    }

    func testDetectsProFromClaudeProCapability() async throws {
        let plan = try await planHint(forCapabilities: ["chat", "claude_pro"])
        XCTAssertEqual(plan, .pro)
    }

    func testDetectsMaxFromChatOnlyCapability() async throws {
        // A consumer chat org without `claude_pro` is Max (tier not exposed by API).
        let plan = try await planHint(forCapabilities: ["chat"])
        XCTAssertEqual(plan, .max200)
    }

    func testNonConsumerOrgHasNoPlan() async throws {
        let plan = try await planHint(forCapabilities: ["api"])
        XCTAssertNil(plan)
    }

    func testFetchAccountParsesUuidEmailAndMemberships() async throws {
        let json = """
        {"uuid":"acct-1","email_address":"person@example.com","full_name":"Person Example",
         "memberships":[
           {"role":"user","organization":{"uuid":"org-company","name":"Example Co","capabilities":["chat","raven"]}},
           {"role":"admin","organization":{"uuid":"org-personal","name":"person@example.com's Organization","capabilities":["chat"]}}
         ]}
        """
        MockURLProtocol.requestHandler = { request in
            XCTAssertEqual(request.url?.path, "/api/account")
            let response = HTTPURLResponse(url: request.url!, statusCode: 200,
                                           httpVersion: nil, headerFields: nil)!
            return (response, json.data(using: .utf8)!)
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let service = UsageAPIService(session: URLSession(configuration: config))

        let info = try await service.fetchAccount(sessionKey: "sk-test")

        XCTAssertEqual(info.uuid, "acct-1")
        XCTAssertEqual(info.email, "person@example.com")
        XCTAssertEqual(info.memberships.count, 2)
        XCTAssertEqual(info.memberships[0].uuid, "org-company")
        XCTAssertEqual(info.memberships[0].capabilities, ["chat", "raven"])
    }

    func testFetchAccountMapsAuthFailureToAuthExpired() async {
        MockURLProtocol.requestHandler = { request in
            let response = HTTPURLResponse(url: request.url!, statusCode: 403,
                                           httpVersion: nil, headerFields: nil)!
            return (response, Data("{}".utf8))
        }

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let service = UsageAPIService(session: URLSession(configuration: config))

        do {
            _ = try await service.fetchAccount(sessionKey: "sk-test")
            XCTFail("expected authExpired")
        } catch UsageAPIError.authExpired {
            // expected
        } catch {
            XCTFail("expected authExpired, got \(error)")
        }
    }
}

// MARK: - Mock

final class MockURLProtocol: URLProtocol {
    static var requestHandler: ((URLRequest) throws -> (HTTPURLResponse, Data))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let handler = Self.requestHandler else {
            // Must fail, never "finish" empty: `URLSession.data(for:)` traps on a task
            // that completes with no response and no error, taking the whole test
            // bundle down. Requests still in flight when a tearDown clears the handler
            // land here.
            client?.urlProtocol(self, didFailWithError: URLError(.cancelled))
            return
        }

        do {
            let (response, data) = try handler(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}
}

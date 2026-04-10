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
}

// MARK: - Mock

final class MockURLProtocol: URLProtocol {
    static var requestHandler: ((URLRequest) throws -> (HTTPURLResponse, Data))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let handler = Self.requestHandler else {
            client?.urlProtocolDidFinishLoading(self)
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

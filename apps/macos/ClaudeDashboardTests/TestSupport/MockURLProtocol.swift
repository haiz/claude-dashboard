import Foundation

/// Shared by both test bundles: `ClaudeDashboardTests` and
/// `ClaudeDashboardHelperTests`. Anything else the two bundles share belongs in
/// this directory and nowhere else.
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

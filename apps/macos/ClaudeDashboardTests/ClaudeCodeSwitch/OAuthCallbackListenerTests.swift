import XCTest
@testable import ClaudeDashboard

final class OAuthCallbackListenerTests: XCTestCase {

    /// Hits the listener's local `/callback` with the given query, returns the HTTP body.
    @discardableResult
    private func hit(port: Int, query: String) async throws -> String {
        // The real browser redirects to `localhost` (Claude Code's registered redirect), so
        // test that host, not 127.0.0.1, to prove the listener accepts the browser's callback.
        let url = URL(string: "http://localhost:\(port)/callback?\(query)")!
        let (data, _) = try await URLSession(configuration: .ephemeral).data(from: url)
        return String(data: data, encoding: .utf8) ?? ""
    }

    func testDeliversTheCodeWhenStateMatches() async throws {
        let listener = OAuthCallbackListener(expectedState: "st")
        let port = try listener.start()
        XCTAssertGreaterThan(port, 0)

        async let code = listener.waitForCode(timeout: 5)
        let page = try await hit(port: port, query: "code=abc123&state=st")
        XCTAssertTrue(page.lowercased().contains("claude"), "the browser tab should show a human page")

        let delivered = try await code
        XCTAssertEqual(delivered, "abc123")
        listener.cancel()
    }

    func testRejectsAMismatchedStateAndKeepsWaiting() async throws {
        let listener = OAuthCallbackListener(expectedState: "right")
        let port = try listener.start()

        async let code = listener.waitForCode(timeout: 5)
        _ = try await hit(port: port, query: "code=wrong-grant&state=wrong")
        // The wrong-state hit is ignored; the correct one still resolves the wait.
        _ = try await hit(port: port, query: "code=good-grant&state=right")

        let delivered = try await code
        XCTAssertEqual(delivered, "good-grant")
        listener.cancel()
    }

    func testTimesOutWhenNoCallbackArrives() async throws {
        let listener = OAuthCallbackListener(expectedState: "st")
        _ = try listener.start()
        do {
            _ = try await listener.waitForCode(timeout: 0.3)
            XCTFail("expected a timeout")
        } catch {
            XCTAssertEqual(error as? OAuthCallbackError, .timedOut)
        }
        listener.cancel()
    }

    func testCancelWakesTheWaiter() async throws {
        let listener = OAuthCallbackListener(expectedState: "st")
        _ = try listener.start()
        async let code = listener.waitForCode(timeout: 5)
        listener.cancel()
        do {
            _ = try await code
            XCTFail("expected cancellation")
        } catch {
            XCTAssertEqual(error as? OAuthCallbackError, .cancelled)
        }
    }
}

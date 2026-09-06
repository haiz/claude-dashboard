import XCTest

/// The override is a test seam, so its validation is the thing that keeps it
/// from being a redirection vector: only plain http on loopback is honoured,
/// and a rejected value falls back silently (a warning would break the
/// byte-exact stderr assertions in contract/helper-cli.md).
final class APIBaseURLTests: XCTestCase {

    func testNoOverrideIsProduction() {
        XCTAssertEqual(APIBaseURL.resolve(nil), "https://claude.ai")
    }

    func testEmptyOverrideIsProduction() {
        XCTAssertEqual(APIBaseURL.resolve(""), "https://claude.ai")
    }

    func testLoopbackIPWithPortIsHonoured() {
        XCTAssertEqual(APIBaseURL.resolve("http://127.0.0.1:52341"), "http://127.0.0.1:52341")
    }

    func testLocalhostWithPortIsHonoured() {
        XCTAssertEqual(APIBaseURL.resolve("http://localhost:8080"), "http://localhost:8080")
    }

    func testLoopbackWithoutPortIsHonoured() {
        XCTAssertEqual(APIBaseURL.resolve("http://127.0.0.1"), "http://127.0.0.1")
    }

    func testPathInOverrideIsDropped() {
        XCTAssertEqual(APIBaseURL.resolve("http://127.0.0.1:9/api/x"), "http://127.0.0.1:9")
    }

    func testHttpsLoopbackIsRejected() {
        XCTAssertEqual(APIBaseURL.resolve("https://127.0.0.1:52341"), "https://claude.ai")
    }

    func testNonLoopbackHostIsRejected() {
        XCTAssertEqual(APIBaseURL.resolve("http://evil.example.com"), "https://claude.ai")
    }

    func testHostThatMerelyStartsWithLoopbackIsRejected() {
        XCTAssertEqual(APIBaseURL.resolve("http://127.0.0.1.evil.com"), "https://claude.ai")
    }

    func testGarbageIsRejected() {
        XCTAssertEqual(APIBaseURL.resolve("not a url at all"), "https://claude.ai")
    }

    func testUserinfoAuthorityIsRejected() {
        // URL(string:) parses userinfo separately from host, so unlike a
        // naive split on ':', url.host here is "127.0.0.1" -- resolve must
        // reject on url.user/url.password being non-nil to match Rust's
        // (conservative) rejection of the same string. See task-2-report.md
        // fix round 1.
        XCTAssertEqual(APIBaseURL.resolve("http://user:pass@127.0.0.1:8080"), "https://claude.ai")
    }

    func testUppercaseSchemeIsRejected() {
        // URL(string:) does not normalize scheme casing, so url.scheme is
        // "HTTP" here; the existing `url.scheme == "http"` comparison is
        // already case-exact and rejects this -- same outcome as Rust's
        // case-sensitive strip_prefix("http://").
        XCTAssertEqual(APIBaseURL.resolve("HTTP://127.0.0.1"), "https://claude.ai")
    }

    func testApiRootAppendsApiSegment() {
        // apiRoot is what every call site composes onto; with no override set
        // in this test process it must be the production root.
        XCTAssertEqual(APIBaseURL.apiRoot, "https://claude.ai/api")
    }
}

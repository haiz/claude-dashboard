import XCTest
@testable import ClaudeDashboard

final class SecurityCLIKeychainTests: XCTestCase {

    func testReadReturnsNilOnStatus44() throws {
        let kc = SecurityCLIKeychain { _, _ in (44, Data()) }
        XCTAssertNil(try kc.read(service: "s", account: "a"))
    }

    func testReadThrowsOnOtherFailure() {
        let kc = SecurityCLIKeychain { _, _ in (51, Data()) }
        XCTAssertThrowsError(try kc.read(service: "s", account: "a")) {
            XCTAssertEqual($0 as? KeychainError, .commandFailed(status: 51))
        }
    }

    func testReadPassesServiceAndAccountAndStripsNewline() throws {
        var seen: [String] = []
        let kc = SecurityCLIKeychain { args, _ in seen = args; return (0, Data("{\"a\":1}\n".utf8)) }
        XCTAssertEqual(try kc.read(service: "Claude Code-credentials", account: "me"), Data("{\"a\":1}".utf8))
        XCTAssertEqual(seen, ["find-generic-password", "-a", "me", "-s", "Claude Code-credentials", "-w"])
    }

    func testDecodesHexOutputForNonASCII() {
        // `security -w` printed {"a":"Việt b"} as hex in a real probe.
        let out = Data("7b2261223a225669e1bb87742062227d\n".utf8)
        XCTAssertEqual(SecurityCLIKeychain.decodePasswordOutput(out), Data(#"{"a":"Việt b"}"#.utf8))
    }

    func testWriteSendsHexOnStdinNeverInArguments() throws {
        var stored = Data()
        var writeArgs: [String] = []
        var writeStdin = ""
        let kc = SecurityCLIKeychain { args, stdin in
            if args == ["-i"] {
                writeArgs = args
                writeStdin = String(decoding: stdin ?? Data(), as: UTF8.self)
                stored = Data(#"{"t":"secret"}"#.utf8)
                return (0, Data())
            }
            return (0, stored + Data("\n".utf8))
        }
        try kc.write(Data(#"{"t":"secret"}"#.utf8), service: "Claude Code-credentials", account: "me")
        XCTAssertEqual(writeArgs, ["-i"])
        XCTAssertFalse(writeStdin.contains("secret"))
        XCTAssertEqual(writeStdin,
            "add-generic-password -U -a \"me\" -s \"Claude Code-credentials\" -X 7b2274223a22736563726574227d\n")
    }

    func testWriteThrowsWhenReadBackDiffers() {
        let kc = SecurityCLIKeychain { args, _ in args == ["-i"] ? (0, Data()) : (0, Data("old\n".utf8)) }
        XCTAssertThrowsError(try kc.write(Data("new".utf8), service: "s", account: "a")) {
            XCTAssertEqual($0 as? KeychainError, .writeNotPersisted)
        }
    }

    /// Opt-in: touches the real login Keychain with a throwaway item.
    /// Run with CLAUDE_DASHBOARD_KEYCHAIN_IT=1.
    func testRealKeychainRoundTrip() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["CLAUDE_DASHBOARD_KEYCHAIN_IT"] == "1")
        let kc = SecurityCLIKeychain()
        let service = "ClaudeDashboard.tests.\(UUID().uuidString)"
        let account = NSUserName()
        defer { _ = try? SecurityCLIKeychain.runSecurity(["delete-generic-password", "-a", account, "-s", service], nil) }
        XCTAssertNil(try kc.read(service: service, account: account))
        let payload = Data(#"{"name":"Việt"}"#.utf8)
        try kc.write(payload, service: service, account: account)
        XCTAssertEqual(try kc.read(service: service, account: account), payload)
    }
}

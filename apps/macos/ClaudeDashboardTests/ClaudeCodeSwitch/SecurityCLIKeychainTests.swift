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

    /// `n` bytes starting with `{`, so `decodePasswordOutput` never mistakes it for hex.
    private static func jsonLike(bytes n: Int) -> Data {
        Data("{".utf8) + Data(repeating: 0x41, count: n - 1)
    }

    /// Length of `add-generic-password -U -a "<account>" -s "<service>" -X ` for plain names.
    private func linePrefixLength(account: String, service: String) -> Int {
        "add-generic-password -U -a \"\(account)\" -s \"\(service)\" -X ".count
    }

    func testWriteOverLineLimitUsesArgvNotInteractive() throws {
        let prefix = linePrefixLength(account: "ab", service: "s")
        let payload = Self.jsonLike(bytes: (SecurityCLIKeychain.interactiveLineLimit - prefix) / 2 + 1)
        var calls: [(args: [String], stdin: Data?)] = []
        let kc = SecurityCLIKeychain { args, stdin in
            calls.append((args, stdin))
            return args.first == "find-generic-password" ? (0, payload + Data("\n".utf8)) : (0, Data())
        }
        try kc.write(payload, service: "s", account: "ab")
        let write = try XCTUnwrap(calls.first)
        XCTAssertEqual(Array(write.args.prefix(7)), ["add-generic-password", "-U", "-a", "ab", "-s", "s", "-X"])
        XCTAssertEqual(write.args.count, 8)
        XCTAssertEqual(write.args[7], payload.map { String(format: "%02x", $0) }.joined())
        XCTAssertNil(write.stdin)
        XCTAssertFalse(calls.contains { $0.args == ["-i"] })
    }

    func testWriteAtLineLimitUsesInteractive() throws {
        let prefix = linePrefixLength(account: "ab", service: "s")
        XCTAssertEqual((SecurityCLIKeychain.interactiveLineLimit - prefix) % 2, 0)
        let payload = Self.jsonLike(bytes: (SecurityCLIKeychain.interactiveLineLimit - prefix) / 2)
        var stdinLine = ""
        let kc = SecurityCLIKeychain { args, stdin in
            if args == ["-i"] { stdinLine = String(decoding: stdin ?? Data(), as: UTF8.self); return (0, Data()) }
            return (0, payload + Data("\n".utf8))
        }
        try kc.write(payload, service: "s", account: "ab")
        XCTAssertEqual(stdinLine.count, SecurityCLIKeychain.interactiveLineLimit + 1)
        XCTAssertTrue(stdinLine.hasSuffix("\n"))
    }

    func testWriteThrowsOnWriteCommandFailure() {
        let kc = SecurityCLIKeychain { _, _ in (1, Data()) }
        XCTAssertThrowsError(try kc.write(Data("x".utf8), service: "s", account: "a")) {
            XCTAssertEqual($0 as? KeychainError, .commandFailed(status: 1))
        }
    }

    func testDeletePassesServiceAndAccountAndTreats44AsSuccess() throws {
        for status: Int32 in [0, 44] {
            var seen: [[String]] = []
            let kc = SecurityCLIKeychain { args, stdin in XCTAssertNil(stdin); seen.append(args); return (status, Data()) }
            try kc.delete(service: "ClaudeDashboard.cc-vault", account: "id-1")
            XCTAssertEqual(seen, [["delete-generic-password", "-a", "id-1", "-s", "ClaudeDashboard.cc-vault"]])
        }
    }

    func testDeleteThrowsOnOtherFailure() {
        let kc = SecurityCLIKeychain { _, _ in (51, Data()) }
        XCTAssertThrowsError(try kc.delete(service: "s", account: "a")) {
            XCTAssertEqual($0 as? KeychainError, .commandFailed(status: 51))
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
        // Over interactiveLineLimit: exercises the argv path with a ~4.6 KB payload.
        let big = Data((#"{"name":"Việt","pad":""#
            + String(repeating: "x", count: 4600) + #""}"#).utf8)
        try kc.write(big, service: service, account: account)
        XCTAssertEqual(try kc.read(service: service, account: account), big)
    }
}

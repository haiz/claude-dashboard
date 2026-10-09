# Claude Code Account Switch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A one-click Switch button that moves Claude Code (single `~/.claude` config dir) to another dashboard account by swapping Keychain credentials, so each account signs in with `/login` once instead of daily.

**Architecture:** Four small units behind protocols: a `security`-CLI Keychain transport, a slot for Claude Code's `Claude Code-credentials` entry, a per-account vault in the app's own Keychain items, and a writer for the `oauthAccount` key of `~/.claude.json`. `ClaudeCodeSwitcher` orchestrates capture (every refresh) and switch; `DashboardViewModel` owns it and the views call it.

**Tech Stack:** Swift 5, SwiftUI, Foundation (`Process`, `JSONSerialization`), XCTest, XcodeGen. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-08-claude-code-account-switch-design.md`

## Global Constraints

- Scope: `apps/macos/ClaudeDashboard` and `apps/macos/ClaudeDashboardTests` only. Linux, Windows and the helper CLI are untouched.
- Deployment target macOS 13.0, Swift 5.0, no external dependencies.
- Claude Code's entry: service `Claude Code-credentials`, account `NSUserName()`. Only the `claudeAiOauth` key is replaced; every other key (`mcpOAuth`) is preserved.
- Vault items: service `ClaudeDashboard.cc-vault`, account `Account.id.uuidString`, JSON `{"claudeAiOauth": {...}, "oauthAccount": {...}}`.
- All Keychain access via `/usr/bin/security`; writes as hex (`-X`) on stdin of `security -i` when the command line is at most 4032 characters, otherwise as argv `add-generic-password -U -a <acct> -s <svc> -X <hex>` (Claude Code 2.1.294's own rule: `security -i` truncates lines near 4096 bytes and would overwrite the item with a prefix). Exit status 44 means "item not found".
- In `~/.claude.json` only the `oauthAccount` key changes; other keys and the file's POSIX permissions are preserved; the write is atomic.
- Refresh window for the pre-switch wait: from 60 s past `expiresAt` to 300 s before it; poll every 3 s, at most 10 polls, then continue.
- Never log or display a token value. Diagnostics use `print("[ClaudeCodeSwitcher] ...")`.
- Under XCTest the app must not build the real switcher: `ClaudeCodeSwitcher.live(isRunningTests: true)` returns nil.
- The four intentional `AccountCard` gauge details stay unchanged (two circles, segmented countdown, single-letter labels, animal emoji overlay).
- New test fakes live in `ClaudeDashboardTests/ClaudeCodeSwitch/`, not `TestSupport/` (that folder is also compiled into the helper test bundle, which cannot see app types).
- Commit messages carry no Claude/Anthropic attribution.
- After adding any file run `cd apps/macos && xcodegen generate` before building.

## Review Focus

1. Claude Code is signed in to an account that is not in the dashboard: the switch must refuse, not discard that account's refresh token. Pinned in Task 5 (`testSwitchRefusesWhenActiveAccountIsNotInDashboard`).
2. Email case differs between `~/.claude.json` and the dashboard (`Frontend@…` vs `frontend@…`), or an account has no email: match case-insensitively; a nil email never matches. Pinned in Task 5 (`testCaptureMatchesEmailCaseInsensitively`, `testCaptureIgnoresAccountWithoutEmail`).
3. `oauthAccount` contains non-ASCII (a Vietnamese display name): `security -w` prints hex, which must be decoded, or the vault reads garbage. Pinned in Task 2 (`testDecodesHexOutputForNonASCII`).
4. Switching away from an idle account whose access token expired hours ago: no 30-second wait. Pinned in Task 5 (`testSwitchDoesNotWaitWhenTokenExpiredLongAgo`).
5. `~/.claude.json` has hundreds of other keys and mode 0600: they and the mode survive the write. Pinned in Task 4 (`testWritePreservesOtherKeysAndPermissions`).

## File Structure

| File | Responsibility |
|---|---|
| Create `ClaudeDashboard/Models/ClaudeCodeCredential.swift` | `OAuthCredential`, `VaultEntry`, `OAuthAccountJSON` value types (pure) |
| Create `ClaudeDashboard/Services/ClaudeCodeSwitch/SecurityCLIKeychain.swift` | `KeychainStoring` protocol, `KeychainError`, `SecurityCLIKeychain` |
| Create `ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeCodeSlot.swift` | `ClaudeCodeCredentialSlot` protocol, `KeychainClaudeCodeSlot` |
| Create `ClaudeDashboard/Services/ClaudeCodeSwitch/CredentialVault.swift` | `CredentialVaulting` protocol, `KeychainCredentialVault` |
| Create `ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeConfigFile.swift` | `ClaudeConfigAccountFile` protocol, `ClaudeConfigFile` |
| Create `ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeCodeSwitcher.swift` | `SwitchAvailability`, `SwitchError`, `CaptureResult`, `ClaudeCodeSwitcher` |
| Modify `ClaudeDashboard/ViewModels/DashboardViewModel.swift` | own the switcher, publish availability and message, `switchClaudeCode(to:)` |
| Modify `ClaudeDashboard/Views/AccountCard.swift` | Switch button |
| Modify `ClaudeDashboard/Views/AccountPane.swift` | Switch action row |
| Modify `ClaudeDashboard/Views/DashboardPane.swift`, `MenuBarPopover.swift` | pass availability and action; show message alert |
| Create `ClaudeDashboardTests/ClaudeCodeSwitch/InMemoryKeychain.swift` | fake `KeychainStoring` |
| Create `ClaudeDashboardTests/ClaudeCodeSwitch/*Tests.swift` | one test file per unit |
| Modify `CLAUDE.md` | document the switcher |

---

### Task 1: Credential value types

**Files:**
- Create: `apps/macos/ClaudeDashboard/Models/ClaudeCodeCredential.swift`
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeCodeCredentialTests.swift`

**Interfaces:**
- Produces:
  - `struct OAuthCredential: Equatable { let json: Data; let refreshToken: String; let expiresAt: Date?; let refreshTokenExpiresAt: Date?; init?(json: Data); init?(object: [String: Any]); var object: [String: Any]; var isBlank: Bool }`
  - `struct VaultEntry: Equatable { let oauth: OAuthCredential; let oauthAccount: Data }`
  - `enum OAuthAccountJSON { static func canonical(_ object: Any) -> Data?; static func object(_ json: Data) -> [String: Any]?; static func email(_ json: Data) -> String? }`

- [ ] **Step 1: Write the failing test**

```swift
import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeCredentialTests: XCTestCase {

    func testParsesDecisionFieldsAndKeepsUnknownOnes() throws {
        let json = Data("""
        {"accessToken":"a","refreshToken":"r1","expiresAt":1760000000000,
         "refreshTokenExpiresAt":1762000000000,"scopes":["user:inference"],"futureField":7}
        """.utf8)
        let c = try XCTUnwrap(OAuthCredential(json: json))
        XCTAssertEqual(c.refreshToken, "r1")
        XCTAssertEqual(c.expiresAt, Date(timeIntervalSince1970: 1_760_000_000))
        XCTAssertEqual(c.refreshTokenExpiresAt, Date(timeIntervalSince1970: 1_762_000_000))
        XCTAssertEqual(c.object["futureField"] as? Int, 7)
        XCTAssertFalse(c.isBlank)
    }

    func testBlankedCredentialIsBlankAndHasNoExpiry() throws {
        let c = try XCTUnwrap(OAuthCredential(json: Data(#"{"accessToken":"","refreshToken":"","expiresAt":0}"#.utf8)))
        XCTAssertTrue(c.isBlank)
        XCTAssertNil(c.expiresAt)
    }

    func testKeyOrderDoesNotAffectEquality() {
        let a = OAuthCredential(json: Data(#"{"refreshToken":"r","expiresAt":1}"#.utf8))
        let b = OAuthCredential(json: Data(#"{"expiresAt":1,"refreshToken":"r"}"#.utf8))
        XCTAssertEqual(a, b)
    }

    func testRejectsNonObject() {
        XCTAssertNil(OAuthCredential(json: Data("[1,2]".utf8)))
        XCTAssertNil(OAuthCredential(json: Data("not json".utf8)))
    }

    func testOAuthAccountEmail() {
        let json = OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "displayName": "Việt"])!
        XCTAssertEqual(OAuthAccountJSON.email(json), "a@b.co")
        XCTAssertNil(OAuthAccountJSON.email(OAuthAccountJSON.canonical(["x": 1])!))
        XCTAssertNil(OAuthAccountJSON.canonical([1, 2]))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeCredentialTests`
Expected: build FAIL, "cannot find 'OAuthCredential' in scope".

- [ ] **Step 3: Write minimal implementation**

```swift
import Foundation

/// The `claudeAiOauth` object from Claude Code's Keychain entry, carried verbatim.
///
/// Only the fields the switcher decides on are parsed; `json` keeps every field, so a
/// write never drops one a newer Claude Code added.
///
///     let c = OAuthCredential(json: data)   // nil unless `data` is a JSON object
///     c?.isBlank                             // true once Claude Code wiped it
struct OAuthCredential: Equatable {
    /// The object serialized with sorted keys, so equal credentials compare equal.
    let json: Data
    let refreshToken: String
    let expiresAt: Date?
    let refreshTokenExpiresAt: Date?

    init?(json: Data) {
        guard let object = (try? JSONSerialization.jsonObject(with: json)) as? [String: Any] else {
            return nil
        }
        self.init(object: object)
    }

    init?(object: [String: Any]) {
        guard let canonical = OAuthAccountJSON.canonical(object) else { return nil }
        self.json = canonical
        self.refreshToken = object["refreshToken"] as? String ?? ""
        self.expiresAt = Self.date(object["expiresAt"])
        self.refreshTokenExpiresAt = Self.date(object["refreshTokenExpiresAt"])
    }

    /// The object form, for embedding in a larger JSON document.
    var object: [String: Any] { OAuthAccountJSON.object(json) ?? [:] }

    /// Claude Code blanks the entry after a failed refresh: empty tokens, `expiresAt` 0.
    var isBlank: Bool { refreshToken.isEmpty }

    /// Claude Code writes epoch milliseconds; 0 and absent both mean "none".
    private static func date(_ value: Any?) -> Date? {
        guard let ms = (value as? NSNumber)?.doubleValue, ms > 0 else { return nil }
        return Date(timeIntervalSince1970: ms / 1000)
    }
}

/// What the app keeps per account: the credential, plus the `oauthAccount` object that
/// `~/.claude.json` must carry while that credential is active. Both verbatim.
struct VaultEntry: Equatable {
    let oauth: OAuthCredential
    /// Canonical JSON (see `OAuthAccountJSON.canonical`).
    let oauthAccount: Data
}

/// Helpers for JSON objects kept as canonical `Data`.
enum OAuthAccountJSON {
    /// Sorted-key, unescaped-slash serialization; nil unless `object` is a dictionary.
    static func canonical(_ object: Any) -> Data? {
        guard object is [String: Any] else { return nil }
        return try? JSONSerialization.data(
            withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes])
    }

    static func object(_ json: Data) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: json)) as? [String: Any]
    }

    /// The `emailAddress` of an `oauthAccount` object.
    static func email(_ json: Data) -> String? {
        object(json)?["emailAddress"] as? String
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeCredentialTests`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Models/ClaudeCodeCredential.swift apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeCodeCredentialTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): Claude Code credential value types"
```

---

### Task 2: `security`-CLI Keychain transport

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/SecurityCLIKeychain.swift`
- Create: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/InMemoryKeychain.swift`
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/SecurityCLIKeychainTests.swift`

**Interfaces:**
- Produces:
  - `protocol KeychainStoring { func read(service: String, account: String) throws -> Data?; func write(_ data: Data, service: String, account: String) throws }`
  - `enum KeychainError: Error, Equatable { case commandFailed(status: Int32); case writeNotPersisted }`
  - `struct SecurityCLIKeychain: KeychainStoring { typealias Runner = (_ arguments: [String], _ stdin: Data?) throws -> (status: Int32, stdout: Data); init(run: @escaping Runner = SecurityCLIKeychain.runSecurity); static func decodePasswordOutput(_ out: Data) -> Data }`
  - Test fake: `final class InMemoryKeychain: KeychainStoring { var items: [String: Data]; var failWrites: Bool }`, key `"\(service)|\(account)"`.

- [ ] **Step 1: Write the failing tests**

`InMemoryKeychain.swift`:

```swift
@testable import ClaudeDashboard
import Foundation

/// In-memory `KeychainStoring`. `failWrites` makes every write throw, for rollback tests.
final class InMemoryKeychain: KeychainStoring {
    var items: [String: Data] = [:]
    var failWrites = false

    func read(service: String, account: String) throws -> Data? {
        items["\(service)|\(account)"]
    }

    func write(_ data: Data, service: String, account: String) throws {
        if failWrites { throw KeychainError.commandFailed(status: 1) }
        items["\(service)|\(account)"] = data
    }
}
```

`SecurityCLIKeychainTests.swift`:

```swift
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/SecurityCLIKeychainTests`
Expected: build FAIL, "cannot find type 'KeychainStoring' in scope".

- [ ] **Step 3: Write minimal implementation**

```swift
import Foundation

/// Generic-password items in the login Keychain, addressed by (service, account).
protocol KeychainStoring {
    /// nil when the item does not exist.
    func read(service: String, account: String) throws -> Data?
    func write(_ data: Data, service: String, account: String) throws
}

enum KeychainError: Error, Equatable {
    case commandFailed(status: Int32)
    /// The write reported success but reading the item back returned other bytes.
    case writeNotPersisted
}

/// Keychain access through `/usr/bin/security`, the tool Claude Code itself uses.
///
/// Claude Code's entry trusts `security` in its access list, so going through it reads
/// and updates `Claude Code-credentials` without an access prompt; a `SecItem` call from
/// this app would prompt. Writes go to `security -i` on stdin with hex data (`-X`), so no
/// secret ever appears in a process argument list.
///
///     let kc = SecurityCLIKeychain()
///     try kc.write(data, service: "ClaudeDashboard.cc-vault", account: id.uuidString)
///     let back = try kc.read(service: "ClaudeDashboard.cc-vault", account: id.uuidString)
struct SecurityCLIKeychain: KeychainStoring {
    /// Runs `/usr/bin/security` with `arguments`, feeding `stdin`; returns status and stdout.
    typealias Runner = (_ arguments: [String], _ stdin: Data?) throws -> (status: Int32, stdout: Data)

    /// `security` exits with this when the item does not exist.
    static let itemNotFoundStatus: Int32 = 44

    private let run: Runner

    init(run: @escaping Runner = SecurityCLIKeychain.runSecurity) {
        self.run = run
    }

    func read(service: String, account: String) throws -> Data? {
        let result = try run(["find-generic-password", "-a", account, "-s", service, "-w"], nil)
        if result.status == Self.itemNotFoundStatus { return nil }
        guard result.status == 0 else { throw KeychainError.commandFailed(status: result.status) }
        return Self.decodePasswordOutput(result.stdout)
    }

    func write(_ data: Data, service: String, account: String) throws {
        let line = "add-generic-password -U -a \(Self.quote(account)) -s \(Self.quote(service)) -X \(Self.hex(data))\n"
        let result = try run(["-i"], Data(line.utf8))
        guard result.status == 0 else { throw KeychainError.commandFailed(status: result.status) }
        // `security -i` can report success for a command it rejected; read back to be sure.
        guard try read(service: service, account: account) == data else {
            throw KeychainError.writeNotPersisted
        }
    }

    /// `security -w` prints the password and a newline, or, when the bytes are not
    /// printable ASCII, the bytes as hex. JSON always contains `{`, so an all-hex line
    /// is never a plain JSON password.
    static func decodePasswordOutput(_ out: Data) -> Data {
        var text = String(decoding: out, as: UTF8.self)
        if text.hasSuffix("\n") { text.removeLast() }
        if !text.isEmpty, text.count % 2 == 0, text.allSatisfy(\.isHexDigit),
           let bytes = unhex(text) {
            return bytes
        }
        return Data(text.utf8)
    }

    static func runSecurity(_ arguments: [String], _ stdin: Data?) throws -> (status: Int32, stdout: Data) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = arguments
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        let input = Pipe()
        process.standardInput = input
        try process.run()
        if let stdin { input.fileHandleForWriting.write(stdin) }
        try input.fileHandleForWriting.close()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, data)
    }

    private static func quote(_ value: String) -> String {
        "\"" + value.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    private static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    private static func unhex(_ text: String) -> Data? {
        var bytes = Data(capacity: text.count / 2)
        var index = text.startIndex
        while index < text.endIndex {
            let next = text.index(index, offsetBy: 2)
            guard let byte = UInt8(text[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        return bytes
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/SecurityCLIKeychainTests`
Expected: PASS, `testRealKeychainRoundTrip` skipped.
Then once, locally: `CLAUDE_DASHBOARD_KEYCHAIN_IT=1 xcodebuild test ... -only-testing:ClaudeDashboardTests/SecurityCLIKeychainTests/testRealKeychainRoundTrip`. Expected: PASS. If the environment variable does not reach the test process, set it in the scheme's Test action environment for that run instead and revert afterwards.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/SecurityCLIKeychain.swift apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): Keychain transport over the security CLI"
```

---

### Task 3: Claude Code slot and credential vault

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeCodeSlot.swift`
- Create: `apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/CredentialVault.swift`
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeCodeSlotTests.swift`

**Interfaces:**
- Consumes: `KeychainStoring`, `InMemoryKeychain` (Task 2); `OAuthCredential`, `VaultEntry`, `OAuthAccountJSON` (Task 1).
- Produces:
  - `protocol ClaudeCodeCredentialSlot { func readOAuth() throws -> OAuthCredential?; func writeOAuth(_ credential: OAuthCredential) throws }`
  - `struct KeychainClaudeCodeSlot: ClaudeCodeCredentialSlot { static let service = "Claude Code-credentials"; init(keychain: KeychainStoring, account: String) }`
  - `protocol CredentialVaulting { func load(_ accountId: UUID) throws -> VaultEntry?; func save(_ entry: VaultEntry, for accountId: UUID) throws }`
  - `struct KeychainCredentialVault: CredentialVaulting { static let service = "ClaudeDashboard.cc-vault"; init(keychain: KeychainStoring) }`

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeSlotTests: XCTestCase {

    private let entryKey = "Claude Code-credentials|me"

    func testReadReturnsNilWhenEntryMissing() throws {
        let slot = KeychainClaudeCodeSlot(keychain: InMemoryKeychain(), account: "me")
        XCTAssertNil(try slot.readOAuth())
    }

    func testWriteReplacesOnlyClaudeAiOauthAndKeepsMcpOAuth() throws {
        let kc = InMemoryKeychain()
        kc.items[entryKey] = Data(#"{"claudeAiOauth":{"refreshToken":"old"},"mcpOAuth":{"figma":{"accessToken":"f"}}}"#.utf8)
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let new = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"new","expiresAt":5}"#.utf8)))

        try slot.writeOAuth(new)

        let root = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items[entryKey])))
        XCTAssertEqual((root["mcpOAuth"] as? [String: Any])?.keys.sorted(), ["figma"])
        XCTAssertEqual(try slot.readOAuth(), new)
    }

    func testWriteCreatesEntryWhenMissing() throws {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let c = try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"r"}"#.utf8)))
        try slot.writeOAuth(c)
        XCTAssertEqual(try slot.readOAuth(), c)
    }

    func testVaultRoundTripKeyedByAccountId() throws {
        let kc = InMemoryKeychain()
        let vault = KeychainCredentialVault(keychain: kc)
        let id = UUID()
        let entry = VaultEntry(
            oauth: try XCTUnwrap(OAuthCredential(json: Data(#"{"refreshToken":"r"}"#.utf8))),
            oauthAccount: try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "displayName": "Việt"])))

        XCTAssertNil(try vault.load(id))
        try vault.save(entry, for: id)

        XCTAssertNotNil(kc.items["ClaudeDashboard.cc-vault|\(id.uuidString)"])
        XCTAssertEqual(try vault.load(id), entry)
        XCTAssertNil(try vault.load(UUID()))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeSlotTests`
Expected: build FAIL, "cannot find 'KeychainClaudeCodeSlot' in scope".

- [ ] **Step 3: Write minimal implementation**

`ClaudeCodeSlot.swift`:

```swift
import Foundation

/// The credential Claude Code is using right now.
protocol ClaudeCodeCredentialSlot {
    /// nil when the entry or its `claudeAiOauth` key is missing.
    func readOAuth() throws -> OAuthCredential?
    /// Replaces `claudeAiOauth` and leaves every other key untouched.
    func writeOAuth(_ credential: OAuthCredential) throws
}

/// Claude Code's Keychain entry for the default config dir (`~/.claude`).
///
/// The entry also holds `mcpOAuth` (other MCP servers' tokens); a write must keep it.
///
///     let slot = KeychainClaudeCodeSlot(keychain: SecurityCLIKeychain(), account: NSUserName())
struct KeychainClaudeCodeSlot: ClaudeCodeCredentialSlot {
    static let service = "Claude Code-credentials"

    private let keychain: KeychainStoring
    private let account: String

    /// `account` is explicit: Claude Code keys its entry by the login user name, and a
    /// wrong value would silently read and write a different item.
    init(keychain: KeychainStoring, account: String) {
        self.keychain = keychain
        self.account = account
    }

    func readOAuth() throws -> OAuthCredential? {
        guard let root = try readRoot(),
              let oauth = root["claudeAiOauth"] as? [String: Any] else { return nil }
        return OAuthCredential(object: oauth)
    }

    func writeOAuth(_ credential: OAuthCredential) throws {
        var root = try readRoot() ?? [:]
        root["claudeAiOauth"] = credential.object
        let data = try JSONSerialization.data(withJSONObject: root, options: [.withoutEscapingSlashes])
        try keychain.write(data, service: Self.service, account: account)
    }

    private func readRoot() throws -> [String: Any]? {
        guard let data = try keychain.read(service: Self.service, account: account) else { return nil }
        return OAuthAccountJSON.object(data)
    }
}
```

`CredentialVault.swift`:

```swift
import Foundation

/// The app's per-account copies of Claude Code credentials.
protocol CredentialVaulting {
    func load(_ accountId: UUID) throws -> VaultEntry?
    func save(_ entry: VaultEntry, for accountId: UUID) throws
}

/// One Keychain item per dashboard account, holding
/// `{"claudeAiOauth": {...}, "oauthAccount": {...}}`.
///
///     let vault = KeychainCredentialVault(keychain: SecurityCLIKeychain())
///     try vault.save(entry, for: account.id)
struct KeychainCredentialVault: CredentialVaulting {
    static let service = "ClaudeDashboard.cc-vault"

    private let keychain: KeychainStoring

    init(keychain: KeychainStoring) {
        self.keychain = keychain
    }

    func load(_ accountId: UUID) throws -> VaultEntry? {
        guard let data = try keychain.read(service: Self.service, account: accountId.uuidString),
              let root = OAuthAccountJSON.object(data),
              let oauthObject = root["claudeAiOauth"] as? [String: Any],
              let oauth = OAuthCredential(object: oauthObject),
              let accountObject = root["oauthAccount"],
              let oauthAccount = OAuthAccountJSON.canonical(accountObject) else { return nil }
        return VaultEntry(oauth: oauth, oauthAccount: oauthAccount)
    }

    func save(_ entry: VaultEntry, for accountId: UUID) throws {
        let root: [String: Any] = [
            "claudeAiOauth": entry.oauth.object,
            "oauthAccount": OAuthAccountJSON.object(entry.oauthAccount) ?? [:],
        ]
        let data = try JSONSerialization.data(withJSONObject: root, options: [.sortedKeys, .withoutEscapingSlashes])
        try keychain.write(data, service: Self.service, account: accountId.uuidString)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeSlotTests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): Claude Code credential slot and per-account vault"
```

---

### Task 4: `~/.claude.json` `oauthAccount` file

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeConfigFile.swift`
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeConfigFileTests.swift`

**Interfaces:**
- Consumes: `OAuthAccountJSON` (Task 1).
- Produces:
  - `protocol ClaudeConfigAccountFile { func readOAuthAccount() throws -> Data?; func writeOAuthAccount(_ json: Data) throws }`
  - `enum ClaudeConfigFileError: Error, Equatable { case unreadable; case notAnObject }`
  - `struct ClaudeConfigFile: ClaudeConfigAccountFile { init(fileURL: URL); static var defaultURL: URL }`

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class ClaudeConfigFileTests: XCTestCase {

    private var url: URL!

    override func setUp() {
        super.setUp()
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("ClaudeConfigFileTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: url)
        super.tearDown()
    }

    func testReadReturnsNilWhenFileOrKeyMissing() throws {
        XCTAssertNil(try ClaudeConfigFile(fileURL: url).readOAuthAccount())
        try Data(#"{"other":1}"#.utf8).write(to: url)
        XCTAssertNil(try ClaudeConfigFile(fileURL: url).readOAuthAccount())
    }

    func testReadReturnsCanonicalObject() throws {
        try Data(#"{"oauthAccount":{"organizationUuid":"o","emailAddress":"a@b.co"}}"#.utf8).write(to: url)
        let json = try XCTUnwrap(ClaudeConfigFile(fileURL: url).readOAuthAccount())
        XCTAssertEqual(json, OAuthAccountJSON.canonical(["emailAddress": "a@b.co", "organizationUuid": "o"]))
    }

    func testWritePreservesOtherKeysAndPermissions() throws {
        var root: [String: Any] = ["oauthAccount": ["emailAddress": "old@b.co"], "numStartups": 42]
        for i in 0..<300 { root["project\(i)"] = ["allowedTools": ["Bash"], "n": i] }
        try JSONSerialization.data(withJSONObject: root).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        let file = ClaudeConfigFile(fileURL: url)
        let newAccount = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "new@b.co", "displayName": "Việt"]))

        try file.writeOAuthAccount(newAccount)

        let after = try XCTUnwrap(OAuthAccountJSON.object(Data(contentsOf: url)))
        XCTAssertEqual(after.count, root.count)
        XCTAssertEqual(after["numStartups"] as? Int, 42)
        XCTAssertEqual((after["project299"] as? [String: Any])?["n"] as? Int, 299)
        XCTAssertEqual(try file.readOAuthAccount(), newAccount)
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)
    }

    func testWriteRefusesUnreadableFile() throws {
        try Data("not json".utf8).write(to: url)
        let account = try XCTUnwrap(OAuthAccountJSON.canonical(["emailAddress": "a@b.co"]))
        XCTAssertThrowsError(try ClaudeConfigFile(fileURL: url).writeOAuthAccount(account)) {
            XCTAssertEqual($0 as? ClaudeConfigFileError, .unreadable)
        }
        XCTAssertEqual(try Data(contentsOf: url), Data("not json".utf8))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeConfigFileTests`
Expected: build FAIL, "cannot find 'ClaudeConfigFile' in scope".

- [ ] **Step 3: Write minimal implementation**

```swift
import Foundation

/// The `oauthAccount` key of Claude Code's `~/.claude.json`: who Claude Code thinks it
/// is signed in as. Identity metadata only, never a credential.
protocol ClaudeConfigAccountFile {
    /// Canonical JSON of `oauthAccount`; nil when the file or the key is missing.
    func readOAuthAccount() throws -> Data?
    /// Replaces `oauthAccount`, keeping every other key and the file's permissions.
    func writeOAuthAccount(_ json: Data) throws
}

enum ClaudeConfigFileError: Error, Equatable {
    /// The file is missing or is not a JSON object; it is left untouched.
    case unreadable
    /// The value to write is not a JSON object.
    case notAnObject
}

/// `~/.claude.json`. Running Claude Code processes also write this file; the
/// read-modify-rename below keeps that window short (see the spec's error handling).
///
///     let file = ClaudeConfigFile(fileURL: ClaudeConfigFile.defaultURL)
///     try file.writeOAuthAccount(entry.oauthAccount)
struct ClaudeConfigFile: ClaudeConfigAccountFile {
    static var defaultURL: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
    }

    private let fileURL: URL

    init(fileURL: URL) {
        self.fileURL = fileURL
    }

    func readOAuthAccount() throws -> Data? {
        guard let data = try? Data(contentsOf: fileURL),
              let root = OAuthAccountJSON.object(data),
              let account = root["oauthAccount"] else { return nil }
        return OAuthAccountJSON.canonical(account)
    }

    func writeOAuthAccount(_ json: Data) throws {
        guard let account = OAuthAccountJSON.object(json) else { throw ClaudeConfigFileError.notAnObject }
        guard let data = try? Data(contentsOf: fileURL),
              var root = OAuthAccountJSON.object(data) else { throw ClaudeConfigFileError.unreadable }
        let mode = try? FileManager.default.attributesOfItem(atPath: fileURL.path)[.posixPermissions]
        root["oauthAccount"] = account
        let out = try JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .withoutEscapingSlashes])
        try out.write(to: fileURL, options: .atomic)
        if let mode {
            try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: fileURL.path)
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeConfigFileTests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeConfigFile.swift apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeConfigFileTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): read and write oauthAccount in ~/.claude.json"
```

---

### Task 5: `ClaudeCodeSwitcher` (capture, availability, switch)

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeCodeSwitcher.swift`
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeCodeSwitcherTests.swift`

**Interfaces:**
- Consumes: everything from Tasks 1-4; `Account` (`Shared/Account.swift`: `id: UUID`, `email: String?`, memberwise init requires `name`, `chromeProfilePath`, `plan`, `status`, `source`).
- Produces:
  - `enum SwitchAvailability: Equatable { case active, ready, notCaptured, loginExpired, needsLogin }`
  - `enum SwitchError: Error, Equatable { case notCaptured, loginExpired, activeAccountNotInDashboard(email: String), keychain, config, verifyFailed }`
  - `enum CaptureResult: Equatable { case noActiveAccount, blanked(UUID), unchanged(UUID), saved(UUID) }`
  - `final class ClaudeCodeSwitcher: @unchecked Sendable { init(slot:vault:config:now:sleep:); static func live(isRunningTests: Bool) -> ClaudeCodeSwitcher?; func capture(accounts: [Account]) throws -> CaptureResult; func availability(for accounts: [Account]) -> [UUID: SwitchAvailability]; func switchTo(_ target: Account, accounts: [Account]) async throws }`

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest
@testable import ClaudeDashboard

final class ClaudeCodeSwitcherTests: XCTestCase {

    private var kc: InMemoryKeychain!
    private var configURL: URL!
    private var now: Date!
    private var sleeps: [TimeInterval]!
    private var onSleep: (() -> Void)?

    private let frontend = ClaudeCodeSwitcherTests.account("frontend@gotitapp.co")
    private let backend = ClaudeCodeSwitcherTests.account("backend@gotitapp.co")

    override func setUp() {
        super.setUp()
        kc = InMemoryKeychain()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("ClaudeCodeSwitcherTests-\(UUID().uuidString).json")
        now = Date(timeIntervalSince1970: 1_760_000_000)
        sleeps = []
        onSleep = nil
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    // MARK: helpers

    private static func account(_ email: String?) -> Account {
        Account(id: UUID(), name: email ?? "no-email", email: email, chromeProfilePath: "",
                plan: .max5x, status: .active, source: .manual)
    }

    private var slot: KeychainClaudeCodeSlot { KeychainClaudeCodeSlot(keychain: kc, account: "me") }
    private var vault: KeychainCredentialVault { KeychainCredentialVault(keychain: kc) }

    private func makeSwitcher(config: ClaudeConfigAccountFile? = nil) -> ClaudeCodeSwitcher {
        ClaudeCodeSwitcher(
            slot: slot, vault: vault,
            config: config ?? ClaudeConfigFile(fileURL: configURL),
            now: { [unowned self] in self.now },
            sleep: { [unowned self] seconds in self.sleeps.append(seconds); self.onSleep?() })
    }

    private func cred(_ refresh: String, expiresIn: TimeInterval = 8 * 3600,
                      refreshExpiresIn: TimeInterval = 20 * 86400) -> OAuthCredential {
        let ms = { (t: TimeInterval) in Int((self.now.timeIntervalSince1970 + t) * 1000) }
        return OAuthCredential(object: ["accessToken": "a-\(refresh)", "refreshToken": refresh,
                                        "expiresAt": ms(expiresIn), "refreshTokenExpiresAt": ms(refreshExpiresIn)])!
    }

    private func accountJSON(_ email: String) -> Data {
        OAuthAccountJSON.canonical(["emailAddress": email, "organizationName": "MathGPT.ai"])!
    }

    /// Claude Code is signed in as `email` with `credential`.
    private func signIn(_ email: String, _ credential: OAuthCredential) throws {
        try JSONSerialization.data(withJSONObject: ["oauthAccount": OAuthAccountJSON.object(accountJSON(email))!, "projects": [:]])
            .write(to: configURL)
        try slot.writeOAuth(credential)
    }

    // MARK: capture

    func testCaptureSavesActiveCredentialToMatchingAccount() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .saved(frontend.id))
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend, backend]), .unchanged(frontend.id))
    }

    func testCaptureMatchesEmailCaseInsensitively() throws {
        try signIn("Frontend@GotItApp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend]), .saved(frontend.id))
    }

    func testCaptureIgnoresAccountWithoutEmail() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        XCTAssertEqual(try makeSwitcher().capture(accounts: [Self.account(nil)]), .noActiveAccount)
    }

    func testCaptureNeverOverwritesVaultWithBlankedCredential() throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        _ = try makeSwitcher().capture(accounts: [frontend])
        try slot.writeOAuth(OAuthCredential(object: ["accessToken": "", "refreshToken": "", "expiresAt": 0])!)

        XCTAssertEqual(try makeSwitcher().capture(accounts: [frontend]), .blanked(frontend.id))
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    // MARK: availability

    func testAvailability() throws {
        let expired = Self.account("old@gotitapp.co")
        let blank = Self.account("blank@gotitapp.co")
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try vault.save(VaultEntry(oauth: cred("o1", refreshExpiresIn: -1), oauthAccount: accountJSON("old@gotitapp.co")), for: expired.id)
        try signIn("frontend@gotitapp.co", cred("f1"))
        let unknown = Self.account("new@gotitapp.co")

        let map = makeSwitcher().availability(for: [frontend, backend, expired, unknown])
        XCTAssertEqual(map[frontend.id], .active)
        XCTAssertEqual(map[backend.id], .ready)
        XCTAssertEqual(map[expired.id], .loginExpired)
        XCTAssertEqual(map[unknown.id], .notCaptured)

        try signIn("blank@gotitapp.co", OAuthCredential(object: ["refreshToken": ""])!)
        XCTAssertEqual(makeSwitcher().availability(for: [blank])[blank.id], .needsLogin)
    }

    // MARK: switch

    func testSwitchSavesCurrentThenActivatesTargetKeepingMcpOAuth() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        let root = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items["Claude Code-credentials|me"])))
        var withMcp = root; withMcp["mcpOAuth"] = ["figma": ["accessToken": "keep"]]
        kc.items["Claude Code-credentials|me"] = try JSONSerialization.data(withJSONObject: withMcp)
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        // Claude Code rotated frontend's token after the last capture: the switch must save f2, not f1.
        try slot.writeOAuth(cred("f2"))

        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])

        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "backend@gotitapp.co")
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f2")
        let after = try XCTUnwrap(OAuthAccountJSON.object(try XCTUnwrap(kc.items["Claude Code-credentials|me"])))
        XCTAssertNotNil(after["mcpOAuth"])
        XCTAssertEqual(sleeps, [])
    }

    func testSwitchRefusesUncapturedOrExpiredTarget() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .notCaptured) }

        try vault.save(VaultEntry(oauth: cred("b1", refreshExpiresIn: -1), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .loginExpired) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchRefusesWhenActiveAccountIsNotInDashboard() async throws {
        try signIn("stranger@gotitapp.co", cred("s1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        do { try await makeSwitcher().switchTo(backend, accounts: [backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .activeAccountNotInDashboard(email: "stranger@gotitapp.co")) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "s1")
    }

    func testSwitchToActiveAccountIsANoOp() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try await makeSwitcher().switchTo(frontend, accounts: [frontend])
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchWaitsForImminentRefreshThenSavesRotatedToken() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: 120))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        var polls = 0
        onSleep = { [unowned self] in
            polls += 1
            if polls == 2 { try? self.slot.writeOAuth(self.cred("f2")) }   // a session refreshed
        }

        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])

        XCTAssertEqual(sleeps, [3, 3])
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f2")
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    func testSwitchContinuesAfterWaitTimesOut() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: 120))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])
        XCTAssertEqual(sleeps.count, 10)
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
    }

    func testSwitchDoesNotWaitWhenTokenExpiredLongAgo() async throws {
        try signIn("frontend@gotitapp.co", cred("f1", expiresIn: -3 * 3600))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        try await makeSwitcher().switchTo(backend, accounts: [frontend, backend])
        XCTAssertEqual(sleeps, [])
        XCTAssertEqual(try vault.load(frontend.id)?.oauth.refreshToken, "f1")
    }

    func testSwitchRollsBackCredentialWhenConfigWriteFails() async throws {
        struct FailingConfig: ClaudeConfigAccountFile {
            let inner: ClaudeConfigFile
            func readOAuthAccount() throws -> Data? { try inner.readOAuthAccount() }
            func writeOAuthAccount(_ json: Data) throws { throw ClaudeConfigFileError.unreadable }
        }
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        let switcher = makeSwitcher(config: FailingConfig(inner: ClaudeConfigFile(fileURL: configURL)))

        do { try await switcher.switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .config) }
        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "f1")
    }

    func testSwitchReportsKeychainFailureAndChangesNothing() async throws {
        try signIn("frontend@gotitapp.co", cred("f1"))
        try vault.save(VaultEntry(oauth: cred("b1"), oauthAccount: accountJSON("backend@gotitapp.co")), for: backend.id)
        _ = try makeSwitcher().capture(accounts: [frontend])
        kc.failWrites = true

        do { try await makeSwitcher().switchTo(backend, accounts: [frontend, backend]); XCTFail() }
        catch { XCTAssertEqual(error as? SwitchError, .keychain) }
        XCTAssertEqual(OAuthAccountJSON.email(try XCTUnwrap(ClaudeConfigFile(fileURL: configURL).readOAuthAccount())), "frontend@gotitapp.co")
    }

    func testLiveIsNilUnderTests() {
        XCTAssertNil(ClaudeCodeSwitcher.live(isRunningTests: true))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeSwitcherTests`
Expected: build FAIL, "cannot find 'ClaudeCodeSwitcher' in scope".

- [ ] **Step 3: Write minimal implementation**

```swift
import Foundation

/// What the Switch button can do for one account.
enum SwitchAvailability: Equatable {
    /// Claude Code already uses this account.
    case active
    case ready
    /// The vault has no copy: run `/login` once with this account.
    case notCaptured
    /// The vault copy is past `refreshTokenExpiresAt`: run `/login` once.
    case loginExpired
    /// Active, but Claude Code blanked the credential after a failed refresh.
    case needsLogin
}

enum SwitchError: Error, Equatable {
    case notCaptured
    case loginExpired
    /// Claude Code is signed in as an account the dashboard does not know; switching
    /// would discard its refresh token.
    case activeAccountNotInDashboard(email: String)
    case keychain
    /// `~/.claude.json` could not be written; the previous credential was restored.
    case config
    case verifyFailed
}

enum CaptureResult: Equatable {
    case noActiveAccount
    case blanked(UUID)
    case unchanged(UUID)
    case saved(UUID)
}

/// Keeps every account's Claude Code login alive and swaps it into `~/.claude`.
///
/// `capture` runs on every dashboard refresh and copies the active credential into the
/// vault, because Claude Code rotates the refresh token on each refresh and an older copy
/// is dead. `switchTo` saves the active account, then writes the target's credential and
/// `oauthAccount`. Running sessions re-read the Keychain within ~30 s.
///
///     let switcher = ClaudeCodeSwitcher.live(isRunningTests: AppDefaults.isRunningTests())
///     try switcher?.capture(accounts: store.accounts)
///     try await switcher?.switchTo(backend, accounts: store.accounts)
final class ClaudeCodeSwitcher: @unchecked Sendable {
    /// A token this close to expiry, or this little past it, may be refreshing right now.
    static let refreshLead: TimeInterval = 300
    static let refreshGrace: TimeInterval = 60
    static let pollInterval: TimeInterval = 3
    static let maxPolls = 10

    private let slot: ClaudeCodeCredentialSlot
    private let vault: CredentialVaulting
    private let config: ClaudeConfigAccountFile
    private let now: () -> Date
    private let sleep: (TimeInterval) async -> Void

    init(slot: ClaudeCodeCredentialSlot, vault: CredentialVaulting, config: ClaudeConfigAccountFile,
         now: @escaping () -> Date, sleep: @escaping (TimeInterval) async -> Void) {
        self.slot = slot
        self.vault = vault
        self.config = config
        self.now = now
        self.sleep = sleep
    }

    /// The real switcher, or nil under XCTest: the test host is the real app, and its
    /// startup refresh must not read or write the developer's Keychain.
    static func live(isRunningTests: Bool) -> ClaudeCodeSwitcher? {
        guard !isRunningTests else { return nil }
        let keychain = SecurityCLIKeychain()
        return ClaudeCodeSwitcher(
            slot: KeychainClaudeCodeSlot(keychain: keychain, account: NSUserName()),
            vault: KeychainCredentialVault(keychain: keychain),
            config: ClaudeConfigFile(fileURL: ClaudeConfigFile.defaultURL),
            now: Date.init,
            sleep: { seconds in try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000)) })
    }

    // MARK: Capture

    @discardableResult
    func capture(accounts: [Account]) throws -> CaptureResult {
        guard let accountJSON = try config.readOAuthAccount(),
              let email = OAuthAccountJSON.email(accountJSON),
              let account = Self.match(email, in: accounts) else { return .noActiveAccount }
        guard let oauth = try slot.readOAuth(), !oauth.isBlank else { return .blanked(account.id) }
        let entry = VaultEntry(oauth: oauth, oauthAccount: accountJSON)
        if try vault.load(account.id) == entry { return .unchanged(account.id) }
        try vault.save(entry, for: account.id)
        return .saved(account.id)
    }

    // MARK: Availability

    func availability(for accounts: [Account]) -> [UUID: SwitchAvailability] {
        let activeEmail = (try? config.readOAuthAccount()).flatMap { OAuthAccountJSON.email($0) }
        let activeId = activeEmail.flatMap { Self.match($0, in: accounts)?.id }
        let activeBlank = (try? slot.readOAuth())?.isBlank ?? true
        var map: [UUID: SwitchAvailability] = [:]
        for account in accounts {
            if account.id == activeId {
                map[account.id] = activeBlank ? .needsLogin : .active
                continue
            }
            guard let entry = try? vault.load(account.id) else {
                map[account.id] = .notCaptured
                continue
            }
            map[account.id] = isExpired(entry) ? .loginExpired : .ready
        }
        return map
    }

    // MARK: Switch

    func switchTo(_ target: Account, accounts: [Account]) async throws {
        let activeEmail = (try? config.readOAuthAccount()).flatMap { OAuthAccountJSON.email($0) }
        if let activeEmail, Self.match(activeEmail, in: [target]) != nil { return }

        guard let entry = try? vault.load(target.id) else { throw SwitchError.notCaptured }
        guard !isExpired(entry) else { throw SwitchError.loginExpired }

        if let activeEmail, Self.match(activeEmail, in: accounts) == nil,
           let current = try? slot.readOAuth(), !current.isBlank {
            throw SwitchError.activeAccountNotInDashboard(email: activeEmail)
        }

        await waitForImminentRefresh()

        let previous: OAuthCredential?
        do {
            try capture(accounts: accounts)
            previous = try slot.readOAuth()
            try slot.writeOAuth(entry.oauth)
        } catch {
            print("[ClaudeCodeSwitcher] keychain step failed switching to \(target.id): \(error)")
            throw SwitchError.keychain
        }

        do {
            try config.writeOAuthAccount(entry.oauthAccount)
        } catch {
            if let previous { try? slot.writeOAuth(previous) }
            print("[ClaudeCodeSwitcher] config write failed, credential rolled back: \(error)")
            throw SwitchError.config
        }

        guard (try? slot.readOAuth())?.json == entry.oauth.json,
              (try? config.readOAuthAccount()) == entry.oauthAccount else {
            throw SwitchError.verifyFailed
        }
        print("[ClaudeCodeSwitcher] switched to \(target.id)")
    }

    // MARK: Helpers

    /// A session refreshing the active account during a switch would write its newest
    /// token after the app saved the account, and the switch would then overwrite it.
    /// Wait for that refresh to land. A token further past expiry has no session
    /// refreshing it, so there is nothing to wait for; on timeout, continue anyway.
    private func waitForImminentRefresh() async {
        guard let expiresAt = (try? slot.readOAuth())?.expiresAt else { return }
        let current = now()
        guard expiresAt > current.addingTimeInterval(-Self.refreshGrace),
              expiresAt < current.addingTimeInterval(Self.refreshLead) else { return }
        for _ in 0..<Self.maxPolls {
            await sleep(Self.pollInterval)
            if (try? slot.readOAuth())?.expiresAt != expiresAt { return }
        }
    }

    private func isExpired(_ entry: VaultEntry) -> Bool {
        guard let deadline = entry.oauth.refreshTokenExpiresAt else { return false }
        return deadline <= now()
    }

    private static func match(_ email: String, in accounts: [Account]) -> Account? {
        accounts.first { $0.email?.caseInsensitiveCompare(email) == .orderedSame }
    }
}
```

Note on `try?`: Swift 5 flattens `try?` over an optional-returning call (SE-0230), so `try? vault.load(id)` is `VaultEntry?`, nil for both "missing" and "read failed". Both mean "no usable copy".

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/ClaudeCodeSwitcherTests`
Expected: PASS (14 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/ClaudeCodeSwitch/ClaudeCodeSwitcher.swift apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/ClaudeCodeSwitcherTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): ClaudeCodeSwitcher captures and swaps Claude Code logins"
```

---

### Task 6: View model wiring

**Files:**
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` (properties near line 74, `init` near lines 108-134, `refreshAll` near line 226, new MARK after `isActiveClaudeCodeAccount` near line 562)
- Test: `apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/DashboardViewModelSwitchTests.swift`

**Interfaces:**
- Consumes: `ClaudeCodeSwitcher`, `SwitchAvailability`, `SwitchError` (Task 5); `InMemoryKeychain` (Task 2).
- Produces on `DashboardViewModel`:
  - `@Published private(set) var switchAvailability: [UUID: SwitchAvailability]`
  - `@Published var switchMessage: String?`
  - `init(..., ccSwitcher: ClaudeCodeSwitcher? = ClaudeCodeSwitcher.live(isRunningTests: AppDefaults.isRunningTests()))` as the last parameter
  - `func switchClaudeCode(to accountId: UUID) async`
  - `static func message(for error: SwitchError, target: Account) -> String`

- [ ] **Step 1: Write the failing test**

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class DashboardViewModelSwitchTests: XCTestCase {

    private var suite: String!
    private var configURL: URL!

    override func setUp() {
        super.setUp()
        suite = StoreFixture.makeSuiteName()
        configURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("DashboardViewModelSwitchTests-\(UUID().uuidString).json")
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suite)
        try? FileManager.default.removeItem(at: configURL)
        super.tearDown()
    }

    func testRefreshPublishesAvailabilityAndSwitchMovesClaudeCode() async throws {
        let kc = InMemoryKeychain()
        let slot = KeychainClaudeCodeSlot(keychain: kc, account: "me")
        let vault = KeychainCredentialVault(keychain: kc)
        let config = ClaudeConfigFile(fileURL: configURL)
        let switcher = ClaudeCodeSwitcher(slot: slot, vault: vault, config: config,
                                          now: Date.init, sleep: { _ in })
        let store = AccountStore(defaults: try XCTUnwrap(UserDefaults(suiteName: suite)))
        let frontend = Account(id: UUID(), name: "fe", email: "frontend@gotitapp.co", chromeProfilePath: "",
                               plan: .max5x, status: .active, source: .manual)
        let backend = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                              plan: .max5x, status: .active, source: .manual)
        store.addAccount(frontend)
        store.addAccount(backend)
        let soon = Int((Date().timeIntervalSince1970 + 8 * 3600) * 1000)
        let later = Int((Date().timeIntervalSince1970 + 20 * 86400) * 1000)
        try JSONSerialization.data(withJSONObject: ["oauthAccount": ["emailAddress": "frontend@gotitapp.co"]]).write(to: configURL)
        try slot.writeOAuth(OAuthCredential(object: ["refreshToken": "f1", "expiresAt": soon, "refreshTokenExpiresAt": later])!)
        try vault.save(VaultEntry(
            oauth: OAuthCredential(object: ["refreshToken": "b1", "expiresAt": soon, "refreshTokenExpiresAt": later])!,
            oauthAccount: OAuthAccountJSON.canonical(["emailAddress": "backend@gotitapp.co"])!), for: backend.id)

        let vm = DashboardViewModel(accountStore: store,
                                    ccDetector: ClaudeCodeAccountDetector(fileURL: configURL),
                                    ccSwitcher: switcher)
        await vm.refreshAll()

        XCTAssertEqual(vm.switchAvailability[frontend.id], .active)
        XCTAssertEqual(vm.switchAvailability[backend.id], .ready)

        await vm.switchClaudeCode(to: backend.id)

        XCTAssertEqual(try slot.readOAuth()?.refreshToken, "b1")
        XCTAssertEqual(vm.activeClaudeCodeEmail, "backend@gotitapp.co")
        XCTAssertEqual(vm.switchAvailability[backend.id], .active)
        XCTAssertEqual(vm.switchAvailability[frontend.id], .ready)
        XCTAssertEqual(vm.switchMessage?.contains("backend@gotitapp.co"), true)
    }

    func testErrorMessagesNameTheFix() {
        let a = Account(id: UUID(), name: "be", email: "backend@gotitapp.co", chromeProfilePath: "",
                        plan: .max5x, status: .active, source: .manual)
        XCTAssertTrue(DashboardViewModel.message(for: .notCaptured, target: a).contains("/login"))
        XCTAssertTrue(DashboardViewModel.message(for: .activeAccountNotInDashboard(email: "x@y.z"), target: a).contains("x@y.z"))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/macos && xcodegen generate && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/DashboardViewModelSwitchTests`
Expected: build FAIL, "extra argument 'ccSwitcher' in call".

- [ ] **Step 3: Implement**

Add next to `@Published var activeClaudeCodeEmail: String?`:

```swift
    /// Per account: what the Claude Code Switch button can do. Empty when the switcher
    /// is disabled (under XCTest).
    @Published private(set) var switchAvailability: [UUID: SwitchAvailability] = [:]
    /// Outcome of the last Switch, shown once as an alert.
    @Published var switchMessage: String?
```

Add next to `private let ccDetector: ClaudeCodeAccountDetector`:

```swift
    private let ccSwitcher: ClaudeCodeSwitcher?
```

Add as the last `init` parameter, after `cookieProvider`:

```swift
        ccSwitcher: ClaudeCodeSwitcher? = ClaudeCodeSwitcher.live(isRunningTests: AppDefaults.isRunningTests())
```

and in the body after `self.cookieProvider = cookieProvider`:

```swift
        self.ccSwitcher = ccSwitcher
```

In `refreshAll`, directly after `activeClaudeCodeEmail = ccDetector.activeEmail()`:

```swift
        await refreshClaudeCodeSwitchState()
```

After `isActiveClaudeCodeAccount(_:)`:

```swift
    // MARK: - Claude Code Switch

    /// Copies the active Claude Code login into the vault, then recomputes what each
    /// account's Switch button can do. Off the main actor: each step spawns `security`.
    private func refreshClaudeCodeSwitchState() async {
        guard let ccSwitcher else { return }
        let accounts = accountStore.accounts
        switchAvailability = await Task.detached {
            do { try ccSwitcher.capture(accounts: accounts) }
            catch { print("[ClaudeCodeSwitcher] capture failed: \(error)") }
            return ccSwitcher.availability(for: accounts)
        }.value
    }

    func switchClaudeCode(to accountId: UUID) async {
        guard let ccSwitcher,
              let target = accountStore.accounts.first(where: { $0.id == accountId }) else { return }
        let accounts = accountStore.accounts
        do {
            try await Task.detached { try await ccSwitcher.switchTo(target, accounts: accounts) }.value
            switchMessage = "Claude Code now uses \(target.email ?? target.name). "
                + "Running sessions pick it up within about 30 seconds."
        } catch let error as SwitchError {
            switchMessage = Self.message(for: error, target: target)
        } catch {
            switchMessage = "Switch failed: \(error.localizedDescription)"
        }
        activeClaudeCodeEmail = ccDetector.activeEmail()
        switchAvailability = await Task.detached { ccSwitcher.availability(for: accounts) }.value
    }

    static func message(for error: SwitchError, target: Account) -> String {
        let name = target.email ?? target.name
        switch error {
        case .notCaptured:
            return "Run /login once in Claude Code with \(name) so the dashboard can keep its login."
        case .loginExpired:
            return "The saved Claude Code login for \(name) has expired. Run /login once with it."
        case .activeAccountNotInDashboard(let email):
            return "Claude Code is signed in as \(email), which is not in the dashboard. "
                + "Add it first, or its login would be lost."
        case .keychain:
            return "Could not update Claude Code's Keychain entry. Nothing was changed."
        case .config:
            return "Could not update ~/.claude.json. The previous account was restored."
        case .verifyFailed:
            return "The switch could not be confirmed. Check /status in Claude Code."
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass, then the whole bundle**

Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/DashboardViewModelSwitchTests`
Expected: PASS (2 tests).
Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests`
Expected: all PASS. `CommandRunnerTests` is known to be flaky (see project memory); re-run it alone before treating a failure there as real.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift apps/macos/ClaudeDashboardTests/ClaudeCodeSwitch/DashboardViewModelSwitchTests.swift apps/macos/ClaudeDashboard.xcodeproj
git commit -m "feat(macos): wire the Claude Code switcher into the dashboard view model"
```

---

### Task 7: Switch buttons and message alert

**Files:**
- Modify: `apps/macos/ClaudeDashboard/Views/AccountCard.swift` (properties lines 3-12; header button row lines 46-71)
- Modify: `apps/macos/ClaudeDashboard/Views/AccountPane.swift` (`actions`, lines 150-178; `body`)
- Modify: `apps/macos/ClaudeDashboard/Views/DashboardPane.swift` (`AccountCard(` call near line 39)
- Modify: `apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift` (`AccountCard(` call near line 108)

**Interfaces:**
- Consumes: `DashboardViewModel.switchAvailability`, `.switchMessage`, `.switchClaudeCode(to:)` (Task 6).
- Produces: `AccountCard(switchAvailability: SwitchAvailability?, onSwitchClaudeCode: (() -> Void)?)`; `View.claudeCodeSwitchAlert(_ viewModel: DashboardViewModel)`.

This task is SwiftUI layout; it is verified by build plus the manual run in Task 8, not by unit tests.

- [ ] **Step 1: Add the shared help text and alert modifier**

Append to `AccountCard.swift`:

```swift
extension SwitchAvailability {
    /// Tooltip for the Switch control; nil when the control is hidden.
    var switchHelp: String? {
        switch self {
        case .active: return nil
        case .ready: return "Use this account in Claude Code"
        case .notCaptured: return "Run /login once in Claude Code with this account to enable Switch"
        case .loginExpired: return "Saved login expired. Run /login once in Claude Code with this account"
        case .needsLogin: return "Claude Code lost this login. Run /login with this account"
        }
    }
}

extension View {
    /// Shows `viewModel.switchMessage` once, then clears it.
    func claudeCodeSwitchAlert(_ viewModel: DashboardViewModel) -> some View {
        alert("Claude Code", isPresented: Binding(
            get: { viewModel.switchMessage != nil },
            set: { if !$0 { viewModel.switchMessage = nil } }
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(viewModel.switchMessage ?? "")
        }
    }
}
```

- [ ] **Step 2: Add the card button**

In `AccountCard`, add after `var isActiveClaudeCodeAccount: Bool = false`:

```swift
    /// nil hides the Switch button (switcher disabled).
    var switchAvailability: SwitchAvailability? = nil
    var onSwitchClaudeCode: (() -> Void)? = nil
```

In the header `HStack(spacing: 2)`, insert before the terminal `Button`:

```swift
                        if let switchAvailability, let help = switchAvailability.switchHelp {
                            Button {
                                onSwitchClaudeCode?()
                            } label: {
                                Image(systemName: "arrow.left.arrow.right")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .padding(2)
                            }
                            .buttonStyle(.plain)
                            .disabled(switchAvailability != .ready)
                            .help(help)
                        }
```

- [ ] **Step 3: Pass it from both card call sites**

In `DashboardPane.swift` and `MenuBarPopover.swift`, add to each `AccountCard(` call, after `isActiveClaudeCodeAccount: ...,`:

```swift
                                switchAvailability: viewModel.switchAvailability[state.id],
                                onSwitchClaudeCode: { Task { await viewModel.switchClaudeCode(to: state.id) } },
```

Attach `.claudeCodeSwitchAlert(viewModel)` to the outermost view returned by `DashboardPane.body` and by `MenuBarPopover.body`.

- [ ] **Step 4: Add the pane action row**

In `AccountPane.actions`, insert before the `actionRow("Run a command for this account")` row:

```swift
            if let availability = dashboardViewModel.switchAvailability[state.id],
               let help = availability.switchHelp {
                actionRow("Use this account in Claude Code") {
                    Button("Switch") {
                        Task { await dashboardViewModel.switchClaudeCode(to: state.id) }
                    }
                    .disabled(availability != .ready)
                    .help(help)
                }
                Divider()
            }
```

Attach `.claudeCodeSwitchAlert(dashboardViewModel)` to the outer `VStack` in `AccountPane.body`.

- [ ] **Step 5: Build and run the full suite**

Run: `cd apps/macos && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build`
Expected: BUILD SUCCEEDED.
Run: `cd apps/macos && xcodebuild test -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/ClaudeDashboard/Views
git commit -m "feat(macos): Switch button for Claude Code on cards and the account pane"
```

---

### Task 8: Manual verification and docs

**Files:**
- Modify: `CLAUDE.md` (Services Layer list)

- [ ] **Step 1: Document the switcher**

In `CLAUDE.md`, under "### Services Layer", after the `AccountStore` bullet, add:

```markdown
- **ClaudeCodeSwitcher** (`Services/ClaudeCodeSwitch/`) — one-click switch of the account Claude
  Code uses in `~/.claude`. Each refresh copies the active `claudeAiOauth` from the
  `Claude Code-credentials` Keychain entry into a per-account vault (`ClaudeDashboard.cc-vault`),
  since Claude Code rotates refresh tokens; Switch saves the active account, writes the target's
  credential (keeping `mcpOAuth`) and its `oauthAccount` in `~/.claude.json`. All Keychain access
  goes through `/usr/bin/security` (no access prompt; hex on stdin, never argv). Disabled under
  XCTest (`ClaudeCodeSwitcher.live(isRunningTests:)` returns nil). Do not also use the same account
  through another `CLAUDE_CONFIG_DIR`: two copies of one refresh-token chain kill each other.
  Spec: `docs/superpowers/specs/2026-10-08-claude-code-account-switch-design.md`.
```

- [ ] **Step 2: Manual run (needs the user's accounts; do it with the user)**

1. Build and launch the app: `cd apps/macos && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build`, then open the built app.
2. In a terminal, `claude` (default `~/.claude`), `/login` as account A if not already; wait one dashboard refresh. A's card shows the green badge and no Switch button.
3. For a second account B: in the same Claude Code, `/login` as B, wait one refresh (B captured), then click Switch on A's card. Expected alert: "Claude Code now uses A …". A is now captured too.
4. Start `claude` in another terminal, run a prompt, then click Switch on B while it is open. Within ~30 s, `/status` in that running session shows B, and a new prompt succeeds.
5. Switch back to A. Expected: no browser opens; both accounts keep working.
6. Confirm `mcpOAuth` survived: `security find-generic-password -s "Claude Code-credentials" -w | python3 -c 'import json,sys; print(sorted(json.load(sys.stdin)))'` lists `claudeAiOauth` and `mcpOAuth`.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: describe the Claude Code account switcher"
```

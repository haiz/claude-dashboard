# Ping Replaces Run Command (macOS) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the macOS Run Command feature with a Ping that starts an account's 5h/7d usage window through claude.ai, by hand from the account page or automatically for every account whose window is not running.

**Architecture:** `PingService` (in `Shared/`) sends three claude.ai requests with the account's session key and maps the result to a `PingOutcome`. `PingController` (`@MainActor`) owns the auto-ping rule (latch, in-flight set, 10-minute failure cooldown), runs pings and writes `PingLogStore` (SQLite `ping_logs.db`). `DashboardViewModel.refreshAll` asks the controller which accounts to auto-ping and fires them after the refresh. The Run Command subsystem (runner, sheet, classifier, terminal launcher, process registry, command log) is deleted, and `PingMigration` removes its leftovers at launch.

**Tech Stack:** Swift 5, SwiftUI, Combine, Foundation `URLSession`, SQLite3, XCTest, XcodeGen.

**Spec:** `docs/superpowers/specs/2026-10-14-ping-design.md`

## Global Constraints

- Scope: `apps/macos/ClaudeDashboard`, `apps/macos/Shared`, their tests, `CLAUDE.md`, `contract/README.md`. Windows and Linux Run Command stay untouched; `contract/windows.md` is not edited.
- Deployment target macOS 13.0, Swift 5.0. No external dependencies.
- App code reads `AppDefaults.shared`, never `UserDefaults.standard`.
- Ping body: `{"prompt": "Reply with just: ok", "timezone": TimeZone.current.identifier, "attachments": [], "files": [], "rendering_mode": "messages"}`. No `model` field.
- Completion timeout 60 s. Success means the SSE body contains `event: message_start`.
- `PingOutcome` raw values: 0 ok, 1 expired, 2 blocked, 3 failed. `PingTrigger` raw values: 0 manual, 1 auto. Never renumbered.
- `ping_logs.db` keeps the newest 500 rows. Columns: `id, account_id, trig, started, finished, outcome, detail`.
- Auto Ping: one global toggle, key `autoPingEnabled`, default **off**. No per-account setting.
- Auto-ping rule: toggle on, `account.status == .active`, this refresh fetched usage, `five_hour.resets_at == nil || seven_day.resets_at == nil`, not latched, not in flight, not in cooldown. Latch clears only when both `resets_at` are non-nil. Failure cooldown: 10 minutes.
- Settings copy: section "Auto Ping", toggle "Ping when a usage window is not running", footer "Starts a new window for every active account whose 5h or 7d window is not running. Each ping sends one short message from that account."
- Account page copy: row "Start the 5h window now", button "Ping". Always shown; no ping button on cards.
- Sidebar Tools item: "Ping Log".
- Migration deletes `command_logs.db`, `command_logs.db-wal`, `command_logs.db-shm`, every `runCommand_*` and `runCommandTerminal_*` key, and `menuBarShowRunCommand`. It carries nothing over.
- After adding or removing any Swift file: `(cd apps/macos && xcodegen generate)` and commit `apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj` with the task.
- Commit messages carry no Claude/Anthropic attribution.

## Review Focus

1. **Auto Ping turned on while several accounts sit idle**: each is pinged exactly once, not again on every later refresh while the API still says `null` (Task 3 latch test, Task 5 second-refresh test).
2. **A ping that keeps failing (Cloudflare, network) with Auto Ping on**: retried at most once per 10 minutes, never in a tight loop through the refresh that follows the failure (Task 3 cooldown test, Task 5 failure-then-refresh test).
3. **Ping clicked twice quickly, or clicked while an auto ping runs**: one request only (Task 3 in-flight test).
4. **Completion fails after the conversation was created**: the conversation is still deleted, so no stray chat is left in the account (Task 1 429 test).
5. **An account deleted after it was pinged**: the Ping Log still lists its rows as "Removed account" instead of crashing or showing a blank name (Task 6 view model test).

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `apps/macos/Shared/PingService.swift` | Create | `PingOutcome`, `PingResult`, `Pinging`, `PingService` (the three requests and outcome mapping) |
| `apps/macos/ClaudeDashboard/Models/PingLogModels.swift` | Create | `PingTrigger`, `PingLogEntry` (+ its caption) |
| `apps/macos/ClaudeDashboard/Services/Ping/PingLogStore.swift` | Create | SQLite persistence of pings |
| `apps/macos/ClaudeDashboard/Services/Ping/PingController.swift` | Create | `PingTarget`, auto-ping rule, run + log a ping |
| `apps/macos/ClaudeDashboard/Services/Ping/PingMigration.swift` | Create | Launch cleanup of Run Command leftovers |
| `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+Ping.swift` | Create | Glue: targets, manual ping, toggle, firing auto pings |
| `apps/macos/ClaudeDashboard/ViewModels/PingLogViewModel.swift` | Create | Ping Log pane state |
| `apps/macos/ClaudeDashboard/Views/PingLogView.swift` | Create | Ping Log pane |
| `apps/macos/ClaudeDashboardTests/Ping/*.swift` | Create | Tests + `FakePinger` |
| `DashboardViewModel.swift`, `AccountPane.swift`, `GeneralSettingsPane.swift`, `MainWindow.swift`, `SidebarItem.swift`, `AccountCard.swift`, `DashboardPane.swift`, `MenuBarPopover.swift`, `ClaudeDashboardApp.swift`, `AccountStore.swift`, `HelpView.swift` | Modify | Wiring and removal |
| `CommandRunner`, `CommandClassifier`, `TerminalLauncher`, `ProcessTree`, `RunningProcessRegistry`, `RunCommandSheet`, `RunCommandSettings`, `CommandLogModels`, `CommandLogStore`, `CommandLogView`, `CommandLogViewModel` and their tests | Delete | Run Command subsystem |

---

### Task 1: PingService

**Files:**
- Create: `apps/macos/Shared/PingService.swift`
- Test: `apps/macos/ClaudeDashboardTests/Ping/PingServiceTests.swift`

**Interfaces:**
- Consumes: `APIBaseURL.apiRoot` (`Shared/APIBaseURL.swift`), `MockURLProtocol` (`ClaudeDashboardTests/TestSupport/MockURLProtocol.swift`).
- Produces:
  - `enum PingOutcome: Int, CaseIterable, Sendable { case ok = 0, expired = 1, blocked = 2, failed = 3; var label: String }`
  - `struct PingResult: Equatable, Sendable { let outcome: PingOutcome; let detail: String? }`
  - `protocol Pinging: Sendable { func ping(orgId: String, sessionKey: String) async -> PingResult }`
  - `final class PingService: Pinging { init(session: URLSession = .shared); static let prompt: String; static let notDeletedDetail: String }`

- [ ] **Step 1: Write the failing tests**

Create `apps/macos/ClaudeDashboardTests/Ping/PingServiceTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class PingServiceTests: XCTestCase {

    /// One request as the mock saw it. Bodies are read from the stream because
    /// URLSession moves `httpBody` into `httpBodyStream` before a URLProtocol sees it.
    private struct Seen {
        let method: String
        let path: String
        let cookie: String?
        let body: [String: Any]
    }

    private final class Recorder: @unchecked Sendable {
        private let lock = NSLock()
        private var items: [Seen] = []
        func append(_ seen: Seen) { lock.lock(); items.append(seen); lock.unlock() }
        var all: [Seen] { lock.lock(); defer { lock.unlock() }; return items }
    }

    private var recorder: Recorder!
    private var service: PingService!

    override func setUp() {
        super.setUp()
        recorder = Recorder()
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        service = PingService(session: URLSession(configuration: config))
    }

    override func tearDown() {
        MockURLProtocol.requestHandler = nil
        super.tearDown()
    }

    private static func body(of request: URLRequest) -> [String: Any] {
        var data = request.httpBody ?? Data()
        if data.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let n = stream.read(&buffer, maxLength: buffer.count)
                if n <= 0 { break }
                data.append(buffer, count: n)
            }
        }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    }

    /// Routes by step: create (POST, no `/completion`), completion, delete.
    private func respond(
        create: (Int, [String: String], String) = (201, [:], "{}"),
        completion: (Int, [String: String], String) = (200, [:], "event: message_start\ndata: {}\n\nevent: message_stop\ndata: {}\n\n"),
        delete: (Int, [String: String], String) = (204, [:], "")
    ) {
        let recorder = recorder!
        MockURLProtocol.requestHandler = { request in
            let method = request.httpMethod ?? "GET"
            let path = request.url?.path ?? ""
            recorder.append(Seen(method: method, path: path,
                                 cookie: request.value(forHTTPHeaderField: "Cookie"),
                                 body: Self.body(of: request)))
            let reply: (Int, [String: String], String)
            if method == "DELETE" {
                reply = delete
            } else if path.hasSuffix("/completion") {
                reply = completion
            } else {
                reply = create
            }
            let response = HTTPURLResponse(url: request.url!, statusCode: reply.0,
                                           httpVersion: nil, headerFields: reply.1)!
            return (response, Data(reply.2.utf8))
        }
    }

    func testOkPingCreatesCompletesAndDeletesTheConversation() async {
        respond()

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .ok, detail: nil))
        let seen = recorder.all
        XCTAssertEqual(seen.map(\.method), ["POST", "POST", "DELETE"])
        XCTAssertEqual(seen[0].path, "/api/organizations/org-1/chat_conversations")
        let id = seen[0].body["uuid"] as? String
        XCTAssertNotNil(id)
        XCTAssertEqual(seen[0].body["name"] as? String, "")
        XCTAssertEqual(seen[1].path, "/api/organizations/org-1/chat_conversations/\(id!)/completion")
        XCTAssertEqual(seen[1].body["prompt"] as? String, PingService.prompt)
        XCTAssertEqual(seen[1].body["rendering_mode"] as? String, "messages")
        XCTAssertNil(seen[1].body["model"])
        XCTAssertEqual(seen[2].path, "/api/organizations/org-1/chat_conversations/\(id!)")
        XCTAssertTrue(seen.allSatisfy { $0.cookie == "sessionKey=sk-1" })
    }

    func testFailedDeleteStillCountsAsOkWithANote() async {
        respond(delete: (500, [:], ""))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .ok, detail: PingService.notDeletedDetail))
    }

    func test401OnCreateIsExpiredAndStopsThere() async {
        respond(create: (401, [:], "{}"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .expired)
        XCTAssertEqual(recorder.all.count, 1)
    }

    func test403WithJsonBodyIsExpired() async {
        respond(create: (403, [:], #"{"error":{"type":"permission_error"}}"#))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .expired)
    }

    func test403WithCfMitigatedHeaderIsBlocked() async {
        respond(create: (403, ["cf-mitigated": "challenge"], "{}"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .blocked)
    }

    func test403WithHtmlBodyIsBlocked() async {
        respond(create: (403, [:], "  <!DOCTYPE html><title>Just a moment...</title>"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .blocked)
    }

    func test429OnCompletionFailsAndStillDeletesTheConversation() async {
        respond(completion: (429, [:], #"{"error":{"type":"rate_limit_error"}}"#))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .failed, detail: "HTTP 429"))
        XCTAssertEqual(recorder.all.map(\.method), ["POST", "POST", "DELETE"])
    }

    func testStreamWithoutMessageStartFails() async {
        respond(completion: (200, [:], "event: ping\ndata: {}\n\n"))

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result, PingResult(outcome: .failed, detail: "no reply"))
    }

    func testNetworkErrorFails() async {
        MockURLProtocol.requestHandler = { _ in throw URLError(.notConnectedToInternet) }

        let result = await service.ping(orgId: "org-1", sessionKey: "sk-1")

        XCTAssertEqual(result.outcome, .failed)
        XCTAssertNotNil(result.detail)
    }
}
```

- [ ] **Step 2: Regenerate the project and run the tests to verify they fail**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingServiceTests 2>&1 | tail -30
```
Expected: build FAILS with "cannot find 'PingService' in scope".

- [ ] **Step 3: Write the implementation**

Create `apps/macos/Shared/PingService.swift`:

```swift
import Foundation

/// How one ping ended. Raw values are stored in `ping_logs.outcome` and are never renumbered.
enum PingOutcome: Int, CaseIterable, Sendable {
    case ok = 0
    case expired = 1
    case blocked = 2
    case failed = 3

    var label: String {
        switch self {
        case .ok: return "OK"
        case .expired: return "Session expired"
        case .blocked: return "Blocked by Cloudflare"
        case .failed: return "Failed"
        }
    }
}

/// `detail` says why a ping failed, or notes something about an `.ok` one
/// (the conversation could not be deleted).
struct PingResult: Equatable, Sendable {
    let outcome: PingOutcome
    let detail: String?
}

/// Sends one ping. `PingService` is the real one; tests pass a fake.
protocol Pinging: Sendable {
    func ping(orgId: String, sessionKey: String) async -> PingResult
}

/// Starts an account's usage windows by sending one tiny claude.ai message
/// with its session key: create a conversation, complete one prompt, delete it.
/// The completion endpoint is undocumented; `contract/README.md`, "Ping",
/// records the sequence.
///
///     let result = await PingService().ping(orgId: orgId, sessionKey: key)
///     result.outcome   // .ok
final class PingService: Pinging {
    static let prompt = "Reply with just: ok"
    static let notDeletedDetail = "conversation not deleted"
    private static let completionTimeout: TimeInterval = 60
    private static let requestTimeout: TimeInterval = 30

    private let session: URLSession
    private let baseURL = APIBaseURL.apiRoot

    init(session: URLSession = .shared) {
        self.session = session
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        let id = UUID().uuidString.lowercased()
        let collection = "\(baseURL)/organizations/\(orgId)/chat_conversations"

        let created = await send("POST", collection, body: ["uuid": id, "name": ""],
                                 sessionKey: sessionKey, timeout: Self.requestTimeout)
        if let failure = Self.failure(of: created) { return failure }

        let completion = await send("POST", "\(collection)/\(id)/completion", body: [
            "prompt": Self.prompt,
            "timezone": TimeZone.current.identifier,
            "attachments": [],
            "files": [],
            "rendering_mode": "messages",
        ], sessionKey: sessionKey, timeout: Self.completionTimeout)
        let result = Self.completionResult(completion)

        // Best effort, and also after a failed completion, so no stray chat is left.
        let deleted = await send("DELETE", "\(collection)/\(id)", body: nil,
                                 sessionKey: sessionKey, timeout: Self.requestTimeout)
        if result.outcome == .ok, Self.failure(of: deleted) != nil {
            return PingResult(outcome: .ok, detail: Self.notDeletedDetail)
        }
        return result
    }

    // MARK: - Private

    private typealias Response = Result<(HTTPURLResponse, Data), Error>

    private func send(_ method: String, _ urlString: String, body: [String: Any]?,
                      sessionKey: String, timeout: TimeInterval) async -> Response {
        guard let url = URL(string: urlString) else { return .failure(URLError(.badURL)) }
        var request = URLRequest(url: url, timeoutInterval: timeout)
        request.httpMethod = method
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")
        if let body {
            request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }
        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                return .failure(URLError(.badServerResponse))
            }
            return .success((http, data))
        } catch {
            return .failure(error)
        }
    }

    /// nil when the request succeeded (2xx).
    private static func failure(of response: Response) -> PingResult? {
        switch response {
        case .failure(let error):
            return PingResult(outcome: .failed, detail: error.localizedDescription)
        case .success(let (http, data)):
            let status = http.statusCode
            if (200...299).contains(status) { return nil }
            if status == 401 { return PingResult(outcome: .expired, detail: nil) }
            if status == 403 {
                let outcome: PingOutcome = isCloudflare(http, data) ? .blocked : .expired
                return PingResult(outcome: outcome, detail: nil)
            }
            return PingResult(outcome: .failed, detail: "HTTP \(status)")
        }
    }

    /// A Cloudflare challenge carries `cf-mitigated` or answers with an HTML page;
    /// claude.ai's own 403s are JSON.
    private static func isCloudflare(_ http: HTTPURLResponse, _ data: Data) -> Bool {
        if http.value(forHTTPHeaderField: "cf-mitigated") != nil { return true }
        let head = String(decoding: data.prefix(64), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return head.hasPrefix("<")
    }

    private static func completionResult(_ response: Response) -> PingResult {
        if let failure = failure(of: response) { return failure }
        guard case .success(let (_, data)) = response,
              String(decoding: data, as: UTF8.self).contains("event: message_start") else {
            return PingResult(outcome: .failed, detail: "no reply")
        }
        return PingResult(outcome: .ok, detail: nil)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:
```bash
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingServiceTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`, 9 tests.

Then confirm the helper bundle (which also compiles `Shared/`) still builds:
```bash
xcodebuild build-for-testing -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardHelperTests 2>&1 | tail -5
```
Expected: `** TEST BUILD SUCCEEDED **`.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/Shared/PingService.swift apps/macos/ClaudeDashboardTests/Ping/PingServiceTests.swift apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): PingService starts a usage window through claude.ai"
```

---

### Task 2: PingLogStore and log models

**Files:**
- Create: `apps/macos/ClaudeDashboard/Models/PingLogModels.swift`
- Create: `apps/macos/ClaudeDashboard/Services/Ping/PingLogStore.swift`
- Test: `apps/macos/ClaudeDashboardTests/Ping/PingLogStoreTests.swift`

**Interfaces:**
- Consumes: `PingOutcome`, `PingResult` (Task 1).
- Produces:
  - `enum PingTrigger: Int, CaseIterable { case manual = 0, auto = 1; var label: String }`
  - `struct PingLogEntry: Identifiable, Equatable { id: Int64, accountId: UUID, trigger: PingTrigger, startedAt: Date, finishedAt: Date, outcome: PingOutcome, detail: String?; func caption(time: (Date) -> String) -> String }`
  - `actor PingLogStore { init(dbPath: String? = nil, maxEntries: Int = 500); @discardableResult func insert(accountId: UUID, trigger: PingTrigger, startedAt: Date, finishedAt: Date, result: PingResult) -> Int64; func fetch(limit: Int) -> [PingLogEntry]; func latest(for accountId: UUID) -> PingLogEntry?; func clear(); static func defaultDirectory() -> URL; static func defaultDBPath() -> String }`

- [ ] **Step 1: Write the failing tests**

Create `apps/macos/ClaudeDashboardTests/Ping/PingLogStoreTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class PingLogStoreTests: XCTestCase {
    private var dbPath: String!

    override func setUp() {
        super.setUp()
        dbPath = NSTemporaryDirectory() + "ping_logs_\(UUID().uuidString).db"
    }

    override func tearDown() {
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testInsertThenFetchReturnsNewestFirstWithEveryField() async {
        let store = PingLogStore(dbPath: dbPath)
        let a = UUID()
        await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0.addingTimeInterval(1.5),
                           result: PingResult(outcome: .ok, detail: nil))
        await store.insert(accountId: a, trigger: .auto, startedAt: t0.addingTimeInterval(10),
                           finishedAt: t0.addingTimeInterval(11),
                           result: PingResult(outcome: .failed, detail: "HTTP 429"))

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0].trigger, .auto)
        XCTAssertEqual(rows[0].outcome, .failed)
        XCTAssertEqual(rows[0].detail, "HTTP 429")
        XCTAssertEqual(rows[1].accountId, a)
        XCTAssertEqual(rows[1].outcome, .ok)
        XCTAssertNil(rows[1].detail)
        XCTAssertEqual(rows[1].finishedAt.timeIntervalSince1970, t0.timeIntervalSince1970 + 1.5, accuracy: 0.001)
    }

    func testLatestIsPerAccount() async {
        let store = PingLogStore(dbPath: dbPath)
        let a = UUID(), b = UUID()
        await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .ok, detail: nil))
        await store.insert(accountId: b, trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .blocked, detail: nil))

        let latestA = await store.latest(for: a)
        let latestNone = await store.latest(for: UUID())

        XCTAssertEqual(latestA?.outcome, .ok)
        XCTAssertNil(latestNone)
    }

    func testKeepsOnlyTheNewestMaxEntries() async {
        let store = PingLogStore(dbPath: dbPath, maxEntries: 3)
        let a = UUID()
        for i in 0..<5 {
            await store.insert(accountId: a, trigger: .auto, startedAt: t0.addingTimeInterval(Double(i)),
                               finishedAt: t0, result: PingResult(outcome: .ok, detail: "\(i)"))
        }

        let rows = await store.fetch(limit: 10)

        XCTAssertEqual(rows.map(\.detail), ["4", "3", "2"])
    }

    func testClearEmptiesTheLog() async {
        let store = PingLogStore(dbPath: dbPath)
        await store.insert(accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                           result: PingResult(outcome: .ok, detail: nil))

        await store.clear()
        let rows = await store.fetch(limit: 10)

        XCTAssertTrue(rows.isEmpty)
    }

    func testRowsSurviveReopeningTheFile() async {
        let a = UUID()
        do {
            let store = PingLogStore(dbPath: dbPath)
            await store.insert(accountId: a, trigger: .manual, startedAt: t0, finishedAt: t0,
                               result: PingResult(outcome: .expired, detail: nil))
        }
        let reopened = PingLogStore(dbPath: dbPath)
        let latest = await reopened.latest(for: a)
        XCTAssertEqual(latest?.outcome, .expired)
    }

    func testCaptionForOkAndFailedPings() {
        let ok = PingLogEntry(id: 1, accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                              outcome: .ok, detail: nil)
        let noted = PingLogEntry(id: 2, accountId: UUID(), trigger: .manual, startedAt: t0, finishedAt: t0,
                                 outcome: .ok, detail: "conversation not deleted")
        let expired = PingLogEntry(id: 3, accountId: UUID(), trigger: .auto, startedAt: t0, finishedAt: t0,
                                   outcome: .expired, detail: nil)
        let failed = PingLogEntry(id: 4, accountId: UUID(), trigger: .auto, startedAt: t0, finishedAt: t0,
                                  outcome: .failed, detail: "HTTP 429")
        let time: (Date) -> String = { _ in "10:44" }

        XCTAssertEqual(ok.caption(time: time), "Pinged at 10:44")
        XCTAssertEqual(noted.caption(time: time), "Pinged at 10:44 (conversation not deleted)")
        XCTAssertEqual(expired.caption(time: time), "Ping failed at 10:44: Session expired")
        XCTAssertEqual(failed.caption(time: time), "Ping failed at 10:44: Failed, HTTP 429")
    }
}
```

- [ ] **Step 2: Regenerate and run to verify failure**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingLogStoreTests 2>&1 | tail -30
```
Expected: build FAILS with "cannot find 'PingLogStore' in scope".

- [ ] **Step 3: Write the models**

Create `apps/macos/ClaudeDashboard/Models/PingLogModels.swift`:

```swift
import Foundation

/// What started a ping. Raw values are stored in `ping_logs.trig` and are never renumbered.
enum PingTrigger: Int, CaseIterable {
    case manual = 0   // the Ping button on an account page
    case auto = 1     // Auto Ping, after a refresh found a window not running

    var label: String {
        switch self {
        case .manual: return "Manual"
        case .auto: return "Auto"
        }
    }
}

/// One finished ping, as the Ping Log stores it.
struct PingLogEntry: Identifiable, Equatable {
    let id: Int64
    let accountId: UUID
    let trigger: PingTrigger
    let startedAt: Date
    let finishedAt: Date
    let outcome: PingOutcome
    let detail: String?

    /// The line under an account page's Ping button.
    ///
    ///     entry.caption(time: { $0.formatted(date: .omitted, time: .shortened) })
    ///     // "Pinged at 10:44" or "Ping failed at 10:44: Session expired"
    func caption(time: (Date) -> String) -> String {
        let at = time(finishedAt)
        if outcome == .ok {
            return detail.map { "Pinged at \(at) (\($0))" } ?? "Pinged at \(at)"
        }
        let reason = detail.map { "\(outcome.label), \($0)" } ?? outcome.label
        return "Ping failed at \(at): \(reason)"
    }
}
```

- [ ] **Step 4: Write the store**

Create `apps/macos/ClaudeDashboard/Services/Ping/PingLogStore.swift`:

```swift
import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

/// Persists finished pings to `ping_logs.db`, keeping the newest `maxEntries` rows.
/// One row per ping, written when it finishes.
///
///     let store = PingLogStore()
///     await store.insert(accountId: id, trigger: .manual, startedAt: start,
///                        finishedAt: Date(), result: result)
///     await store.latest(for: id)?.outcome   // .ok
actor PingLogStore {
    private var db: OpaquePointer?
    private let maxEntries: Int
    private static let columns = "id, account_id, trig, started, finished, outcome, detail"

    init(dbPath: String? = nil, maxEntries: Int = 500) {
        self.maxEntries = maxEntries
        openDatabase(at: dbPath ?? PingLogStore.defaultDBPath())
        createTable()
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    // MARK: - Write

    @discardableResult
    func insert(accountId: UUID, trigger: PingTrigger, startedAt: Date, finishedAt: Date,
                result: PingResult) -> Int64 {
        let sql = """
        INSERT INTO ping_logs (account_id, trig, started, finished, outcome, detail)
        VALUES (?, ?, ?, ?, ?, ?)
        """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return -1 }
        defer { sqlite3_finalize(stmt) }

        accountId.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, SQLITE_TRANSIENT) }
        sqlite3_bind_int(stmt, 2, Int32(trigger.rawValue))
        sqlite3_bind_double(stmt, 3, startedAt.timeIntervalSince1970)
        sqlite3_bind_double(stmt, 4, finishedAt.timeIntervalSince1970)
        sqlite3_bind_int(stmt, 5, Int32(result.outcome.rawValue))
        if let detail = result.detail {
            detail.withCString { sqlite3_bind_text(stmt, 6, $0, -1, SQLITE_TRANSIENT) }
        } else {
            sqlite3_bind_null(stmt, 6)
        }

        guard sqlite3_step(stmt) == SQLITE_DONE else { return -1 }
        let id = sqlite3_last_insert_rowid(db)
        prune()
        return id
    }

    func clear() {
        sqlite3_exec(db, "DELETE FROM ping_logs", nil, nil, nil)
    }

    // MARK: - Read

    func fetch(limit: Int) -> [PingLogEntry] {
        query("SELECT \(Self.columns) FROM ping_logs ORDER BY id DESC LIMIT ?") { stmt in
            sqlite3_bind_int(stmt, 1, Int32(limit))
        }
    }

    func latest(for accountId: UUID) -> PingLogEntry? {
        query("SELECT \(Self.columns) FROM ping_logs WHERE account_id = ? ORDER BY id DESC LIMIT 1") { stmt in
            accountId.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, SQLITE_TRANSIENT) }
        }.first
    }

    private func query(_ sql: String, bind: (OpaquePointer?) -> Void) -> [PingLogEntry] {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }
        bind(stmt)

        var rows: [PingLogEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let idText = sqlite3_column_text(stmt, 1),
                  let accountId = UUID(uuidString: String(cString: idText)),
                  let trigger = PingTrigger(rawValue: Int(sqlite3_column_int(stmt, 2))),
                  let outcome = PingOutcome(rawValue: Int(sqlite3_column_int(stmt, 5))) else { continue }
            rows.append(PingLogEntry(
                id: sqlite3_column_int64(stmt, 0),
                accountId: accountId,
                trigger: trigger,
                startedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 3)),
                finishedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 4)),
                outcome: outcome,
                detail: sqlite3_column_text(stmt, 6).map { String(cString: $0) }
            ))
        }
        return rows
    }

    // MARK: - Retention

    private func prune() {
        let sql = "DELETE FROM ping_logs WHERE id NOT IN (SELECT id FROM ping_logs ORDER BY id DESC LIMIT ?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_int(stmt, 1, Int32(maxEntries))
        sqlite3_step(stmt)
    }

    // MARK: - Setup

    private func openDatabase(at path: String) {
        let dir = (path as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        guard sqlite3_open(path, &db) == SQLITE_OK else {
            print("[PingLogStore] Failed to open database at \(path)")
            return
        }
    }

    private func createTable() {
        let sql = """
        CREATE TABLE IF NOT EXISTS ping_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT NOT NULL,
            trig INTEGER NOT NULL,
            started REAL NOT NULL,
            finished REAL NOT NULL,
            outcome INTEGER NOT NULL,
            detail TEXT
        )
        """
        sqlite3_exec(db, sql, nil, nil, nil)
    }

    /// `~/Library/Application Support/ClaudeDashboard`, shared with the other logs.
    static func defaultDirectory() -> URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("ClaudeDashboard")
    }

    static func defaultDBPath() -> String {
        defaultDirectory().appendingPathComponent("ping_logs.db").path
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingLogStoreTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`, 6 tests.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/ClaudeDashboard/Models/PingLogModels.swift apps/macos/ClaudeDashboard/Services/Ping/PingLogStore.swift apps/macos/ClaudeDashboardTests/Ping/PingLogStoreTests.swift apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): PingLogStore keeps the newest 500 pings"
```

---

### Task 3: PingController

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/Ping/PingController.swift`
- Create: `apps/macos/ClaudeDashboardTests/Ping/FakePinger.swift`
- Test: `apps/macos/ClaudeDashboardTests/Ping/PingControllerTests.swift`

**Interfaces:**
- Consumes: `Pinging`, `PingResult`, `PingOutcome` (Task 1); `PingLogStore`, `PingTrigger`, `PingLogEntry` (Task 2); `UsageData`, `UsageLimit`, `AccountStatus` (`Shared/`).
- Produces:
  - `struct PingTarget: Equatable, Sendable { let accountId: UUID; let orgId: String; let sessionKey: String }`
  - `@MainActor final class PingController: ObservableObject`
    - `static let enabledKey = "autoPingEnabled"`, `static let failureCooldown: TimeInterval = 600`
    - `init(service: Pinging, logStore: PingLogStore, defaults: UserDefaults, now: @escaping () -> Date = Date.init)`
    - `@Published var isAutoPingEnabled: Bool`
    - `@Published private(set) var inFlight: Set<UUID>`
    - `@Published private(set) var latest: [UUID: PingLogEntry]`
    - `let logStore: PingLogStore`
    - `static func isWindowNotRunning(_ usage: UsageData) -> Bool`
    - `func claimAutoPing(accountId: UUID, status: AccountStatus, freshUsage: UsageData?) -> Bool`
    - `@discardableResult func ping(_ target: PingTarget, trigger: PingTrigger) async -> PingResult?` (nil when a ping for that account is already in flight)
    - `func loadLatest(for accountId: UUID) async`
  - Test helper `final class FakePinger: Pinging` with `var calls: [String]` (org ids) and `func setResult(_:)`.

- [ ] **Step 1: Write the test helper**

Create `apps/macos/ClaudeDashboardTests/Ping/FakePinger.swift`:

```swift
import Foundation
@testable import ClaudeDashboard

/// Records each ping's org id and answers with a settable result.
final class FakePinger: Pinging, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [String] = []
    private var result: PingResult

    init(result: PingResult = PingResult(outcome: .ok, detail: nil)) {
        self.result = result
    }

    var calls: [String] {
        lock.lock(); defer { lock.unlock() }
        return recorded
    }

    func setResult(_ newResult: PingResult) {
        lock.lock(); result = newResult; lock.unlock()
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        lock.lock(); defer { lock.unlock() }
        recorded.append(orgId)
        return result
    }
}

/// Holds every ping until `release()`, so a test can look at the in-flight state.
final class GatedPinger: Pinging, @unchecked Sendable {
    private let lock = NSLock()
    private var waiting: [CheckedContinuation<Void, Never>] = []
    private var count = 0

    var startedCount: Int {
        lock.lock(); defer { lock.unlock() }
        return count
    }

    func ping(orgId: String, sessionKey: String) async -> PingResult {
        await withCheckedContinuation { continuation in
            lock.lock(); waiting.append(continuation); count += 1; lock.unlock()
        }
        return PingResult(outcome: .ok, detail: nil)
    }

    func release() {
        lock.lock(); let all = waiting; waiting = []; lock.unlock()
        all.forEach { $0.resume() }
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `apps/macos/ClaudeDashboardTests/Ping/PingControllerTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class PingControllerTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var dbPath: String!
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)
    private let id = UUID()

    override func setUp() {
        super.setUp()
        suiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suiteName)!
        dbPath = NSTemporaryDirectory() + "ping_ctrl_\(UUID().uuidString).db"
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(atPath: dbPath)
        super.tearDown()
    }

    private func makeController(_ service: Pinging, autoPing: Bool = true) -> PingController {
        defaults.set(autoPing, forKey: PingController.enabledKey)
        return PingController(service: service, logStore: PingLogStore(dbPath: dbPath),
                              defaults: defaults, now: { [unowned self] in self.clock })
    }

    private var target: PingTarget { PingTarget(accountId: id, orgId: "org-1", sessionKey: "sk-1") }

    private func usage(fiveHour: Date?, sevenDay: Date?) -> UsageData {
        UsageData(fiveHour: UsageLimit(utilization: 0, resetsAt: fiveHour),
                  sevenDay: UsageLimit(utilization: 0, resetsAt: sevenDay))
    }

    private var running: Date { clock.addingTimeInterval(3600) }

    // MARK: - The rule

    func testWindowNotRunningWhenEitherResetIsNil() {
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: nil, sevenDay: running)))
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: running, sevenDay: nil)))
        XCTAssertTrue(PingController.isWindowNotRunning(usage(fiveHour: nil, sevenDay: nil)))
        XCTAssertFalse(PingController.isWindowNotRunning(usage(fiveHour: running, sevenDay: running)))
    }

    func testClaimsAnActiveAccountWhoseWindowIsNotRunning() {
        let controller = makeController(FakePinger())
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active,
                                               freshUsage: usage(fiveHour: nil, sevenDay: running)))
    }

    func testNeverClaimsWithoutFreshUsage() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: nil))
    }

    func testNeverClaimsWhenAutoPingIsOff() {
        let controller = makeController(FakePinger(), autoPing: false)
        XCTAssertFalse(controller.isAutoPingEnabled)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    func testAutoPingDefaultsToOff() {
        let controller = PingController(service: FakePinger(), logStore: PingLogStore(dbPath: dbPath),
                                        defaults: defaults)
        XCTAssertFalse(controller.isAutoPingEnabled)
    }

    func testTogglePersists() {
        let controller = makeController(FakePinger(), autoPing: false)
        controller.isAutoPingEnabled = true
        XCTAssertTrue(defaults.bool(forKey: PingController.enabledKey))
    }

    func testNeverClaimsANonActiveAccount() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .expired,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    func testNeverClaimsWhenBothWindowsRun() {
        let controller = makeController(FakePinger())
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: running)))
    }

    // MARK: - Latch

    func testLatchBlocksASecondClaimUntilBothWindowsRunAgain() async {
        let controller = makeController(FakePinger())
        let idle = usage(fiveHour: nil, sevenDay: nil)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        await controller.ping(target, trigger: .auto)

        // The API can still say null for a few seconds after a ping.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        // Only one window running is not enough to clear the latch.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: nil)))

        // Both running: latch cleared; the next reset claims again.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: running, sevenDay: running)))
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
    }

    func testAManualOkPingAlsoLatches() async {
        let controller = makeController(FakePinger())
        await controller.ping(target, trigger: .manual)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))
    }

    // MARK: - Failure cooldown

    func testFailedAutoPingRetriesOnlyAfterTenMinutes() async {
        let pinger = FakePinger(result: PingResult(outcome: .blocked, detail: nil))
        let controller = makeController(pinger)
        let idle = usage(fiveHour: nil, sevenDay: nil)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        await controller.ping(target, trigger: .auto)

        clock = clock.addingTimeInterval(PingController.failureCooldown - 1)
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))

        clock = clock.addingTimeInterval(1)
        XCTAssertTrue(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
        // Claimed once; claiming again right away is blocked until the next failure sets a new cooldown.
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active, freshUsage: idle))
    }

    // MARK: - In flight

    func testASecondPingWhileOneIsInFlightIsSkipped() async {
        let gated = GatedPinger()
        let controller = makeController(gated)
        let first = Task { await controller.ping(target, trigger: .manual) }
        for _ in 0..<1000 where gated.startedCount == 0 { await Task.yield() }
        XCTAssertEqual(gated.startedCount, 1)
        XCTAssertTrue(controller.inFlight.contains(id))
        XCTAssertFalse(controller.claimAutoPing(accountId: id, status: .active,
                                                freshUsage: usage(fiveHour: nil, sevenDay: nil)))

        let second = await controller.ping(target, trigger: .auto)
        XCTAssertNil(second)

        gated.release()
        let firstResult = await first.value
        XCTAssertEqual(firstResult?.outcome, .ok)
        XCTAssertFalse(controller.inFlight.contains(id))
        XCTAssertEqual(gated.startedCount, 1)
    }

    // MARK: - Log

    func testEachPingWritesOneLogRowAndUpdatesLatest() async {
        let pinger = FakePinger(result: PingResult(outcome: .failed, detail: "HTTP 429"))
        let controller = makeController(pinger)

        await controller.ping(target, trigger: .manual)

        let rows = await controller.logStore.fetch(limit: 10)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows[0].accountId, id)
        XCTAssertEqual(rows[0].trigger, .manual)
        XCTAssertEqual(rows[0].detail, "HTTP 429")
        XCTAssertEqual(controller.latest[id]?.outcome, .failed)
    }

    func testLoadLatestReadsTheLog() async {
        let store = PingLogStore(dbPath: dbPath)
        await store.insert(accountId: id, trigger: .auto, startedAt: clock, finishedAt: clock,
                           result: PingResult(outcome: .ok, detail: nil))
        let controller = PingController(service: FakePinger(), logStore: store, defaults: defaults)

        await controller.loadLatest(for: id)

        XCTAssertEqual(controller.latest[id]?.trigger, .auto)
    }
}
```

Note on the in-flight loop: `for _ in 0..<1000 where gated.startedCount == 0` evaluates the `where` per iteration and skips the body once the ping has started, so it is bounded and stops yielding early.

- [ ] **Step 3: Regenerate and run to verify failure**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingControllerTests 2>&1 | tail -30
```
Expected: build FAILS with "cannot find 'PingController' in scope".

- [ ] **Step 4: Write the implementation**

Create `apps/macos/ClaudeDashboard/Services/Ping/PingController.swift`:

```swift
import Foundation

/// What a ping needs to reach claude.ai for one account.
struct PingTarget: Equatable, Sendable {
    let accountId: UUID
    let orgId: String
    let sessionKey: String
}

/// Runs pings and decides auto pings. All state is in memory except the
/// Auto Ping toggle (`autoPingEnabled`, default off).
///
/// Auto-ping rule (spec, "Auto Ping"): a successfully refreshed `.active`
/// account whose 5h or 7d window is not running (`resets_at == nil`) is pinged,
/// once. The latch holds until both windows run again; a failed auto ping may
/// retry after `failureCooldown`.
///
///     if controller.claimAutoPing(accountId: id, status: .active, freshUsage: usage) {
///         await controller.ping(target, trigger: .auto)
///     }
@MainActor
final class PingController: ObservableObject {
    static let enabledKey = "autoPingEnabled"
    static let failureCooldown: TimeInterval = 10 * 60

    @Published var isAutoPingEnabled: Bool {
        didSet { defaults.set(isAutoPingEnabled, forKey: Self.enabledKey) }
    }
    /// Accounts with a ping running; the account page disables its button for them.
    @Published private(set) var inFlight: Set<UUID> = []
    /// Each account's latest ping, from this session or loaded from the log.
    @Published private(set) var latest: [UUID: PingLogEntry] = [:]

    let logStore: PingLogStore
    private let service: Pinging
    private let defaults: UserDefaults
    private let now: () -> Date
    /// Pinged for the current "window not running" episode.
    private var latched: Set<UUID> = []
    /// A latched account whose auto ping failed may be claimed again from this time on.
    private var retryAfter: [UUID: Date] = [:]

    init(service: Pinging, logStore: PingLogStore, defaults: UserDefaults,
         now: @escaping () -> Date = Date.init) {
        self.service = service
        self.logStore = logStore
        self.defaults = defaults
        self.now = now
        self.isAutoPingEnabled = defaults.bool(forKey: Self.enabledKey)
    }

    static func isWindowNotRunning(_ usage: UsageData) -> Bool {
        usage.fiveHour.resetsAt == nil || usage.sevenDay.resetsAt == nil
    }

    /// True when `accountId` should be auto-pinged now; the account is latched as
    /// claimed. `freshUsage` is this refresh's fetch, nil when it failed: an
    /// unreadable window state never pings.
    func claimAutoPing(accountId: UUID, status: AccountStatus, freshUsage: UsageData?) -> Bool {
        guard let usage = freshUsage else { return false }
        guard Self.isWindowNotRunning(usage) else {
            latched.remove(accountId)
            retryAfter[accountId] = nil
            return false
        }
        guard isAutoPingEnabled, status == .active, !inFlight.contains(accountId) else { return false }
        if latched.contains(accountId) {
            guard let until = retryAfter[accountId], now() >= until else { return false }
        }
        latched.insert(accountId)
        retryAfter[accountId] = nil
        return true
    }

    /// Pings and logs one account. nil when a ping for it is already running.
    @discardableResult
    func ping(_ target: PingTarget, trigger: PingTrigger) async -> PingResult? {
        let id = target.accountId
        guard !inFlight.contains(id) else { return nil }
        inFlight.insert(id)
        defer { inFlight.remove(id) }

        let started = now()
        let result = await service.ping(orgId: target.orgId, sessionKey: target.sessionKey)
        let finished = now()

        if result.outcome == .ok {
            // A manual ping started the window too; do not auto-ping it again.
            latched.insert(id)
            retryAfter[id] = nil
        } else if trigger == .auto {
            retryAfter[id] = finished.addingTimeInterval(Self.failureCooldown)
        }

        let rowId = await logStore.insert(accountId: id, trigger: trigger, startedAt: started,
                                          finishedAt: finished, result: result)
        latest[id] = PingLogEntry(id: rowId, accountId: id, trigger: trigger, startedAt: started,
                                  finishedAt: finished, outcome: result.outcome, detail: result.detail)
        return result
    }

    func loadLatest(for accountId: UUID) async {
        if let entry = await logStore.latest(for: accountId) {
            latest[accountId] = entry
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingControllerTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`, 15 tests.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Ping/PingController.swift apps/macos/ClaudeDashboardTests/Ping/FakePinger.swift apps/macos/ClaudeDashboardTests/Ping/PingControllerTests.swift apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): PingController owns the auto-ping rule"
```

---

### Task 4: PingMigration

**Files:**
- Create: `apps/macos/ClaudeDashboard/Services/Ping/PingMigration.swift`
- Test: `apps/macos/ClaudeDashboardTests/Ping/PingMigrationTests.swift`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `enum PingMigration { static func run(defaults: UserDefaults, fileManager: FileManager = .default, logsDirectory: URL) }`. Wired into app launch in Task 7 (not here: until Task 7 the old `CommandLogStore` still opens `command_logs.db`).

- [ ] **Step 1: Write the failing tests**

Create `apps/macos/ClaudeDashboardTests/Ping/PingMigrationTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class PingMigrationTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var dir: URL!

    override func setUp() {
        super.setUp()
        suiteName = StoreFixture.makeSuiteName()
        defaults = UserDefaults(suiteName: suiteName)!
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("PingMigration-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(at: dir)
        super.tearDown()
    }

    private func touch(_ name: String) {
        FileManager.default.createFile(atPath: dir.appendingPathComponent(name).path, contents: Data("x".utf8))
    }

    private func exists(_ name: String) -> Bool {
        FileManager.default.fileExists(atPath: dir.appendingPathComponent(name).path)
    }

    func testRemovesTheCommandLogAndItsSidecars() {
        ["command_logs.db", "command_logs.db-wal", "command_logs.db-shm", "ping_logs.db", "usage_logs.db"].forEach(touch)

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertFalse(exists("command_logs.db"))
        XCTAssertFalse(exists("command_logs.db-wal"))
        XCTAssertFalse(exists("command_logs.db-shm"))
        XCTAssertTrue(exists("ping_logs.db"))
        XCTAssertTrue(exists("usage_logs.db"))
    }

    func testRemovesRunCommandKeysAndKeepsTheRest() {
        let id = UUID().uuidString
        defaults.set("claude -p hi", forKey: "runCommand_\(id)")
        defaults.set(true, forKey: "runCommandTerminal_\(id)")
        defaults.set(true, forKey: "menuBarShowRunCommand")
        defaults.set(7, forKey: "autoRefreshMinutes")
        defaults.set(true, forKey: "autoSwitchEnabled")

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertNil(defaults.object(forKey: "runCommand_\(id)"))
        XCTAssertNil(defaults.object(forKey: "runCommandTerminal_\(id)"))
        XCTAssertNil(defaults.object(forKey: "menuBarShowRunCommand"))
        XCTAssertEqual(defaults.integer(forKey: "autoRefreshMinutes"), 7)
        XCTAssertTrue(defaults.bool(forKey: "autoSwitchEnabled"))
    }

    func testDoesNotTurnAutoPingOn() {
        defaults.set("claude -p hi", forKey: "runCommand_\(UUID().uuidString)")

        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertNil(defaults.object(forKey: "autoPingEnabled"))
    }

    func testSecondRunIsANoOp() {
        touch("command_logs.db")
        defaults.set(true, forKey: "menuBarShowRunCommand")

        PingMigration.run(defaults: defaults, logsDirectory: dir)
        PingMigration.run(defaults: defaults, logsDirectory: dir)

        XCTAssertFalse(exists("command_logs.db"))
        XCTAssertNil(defaults.object(forKey: "menuBarShowRunCommand"))
    }
}
```

- [ ] **Step 2: Regenerate and run to verify failure**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingMigrationTests 2>&1 | tail -30
```
Expected: build FAILS with "cannot find 'PingMigration' in scope".

- [ ] **Step 3: Write the implementation**

Create `apps/macos/ClaudeDashboard/Services/Ping/PingMigration.swift`:

```swift
import Foundation

/// Removes what the Run Command feature left behind: its log database and its
/// UserDefaults keys. Runs at every launch; each step is idempotent, so no
/// "migrated" flag is stored. Nothing is carried over: Auto Ping starts off.
///
///     PingMigration.run(defaults: AppDefaults.shared,
///                       logsDirectory: PingLogStore.defaultDirectory())
enum PingMigration {
    /// `runCommandTerminal_` does not start with `runCommand_`, so both are listed.
    static let legacyKeyPrefixes = ["runCommand_", "runCommandTerminal_"]
    static let legacyKeys: Set<String> = ["menuBarShowRunCommand"]
    static let legacyLogFiles = ["command_logs.db", "command_logs.db-wal", "command_logs.db-shm"]

    static func run(defaults: UserDefaults, fileManager: FileManager = .default, logsDirectory: URL) {
        for file in legacyLogFiles {
            try? fileManager.removeItem(at: logsDirectory.appendingPathComponent(file))
        }
        for key in defaults.dictionaryRepresentation().keys
        where legacyKeys.contains(key) || legacyKeyPrefixes.contains(where: { key.hasPrefix($0) }) {
            defaults.removeObject(forKey: key)
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:
```bash
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingMigrationTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/ClaudeDashboard/Services/Ping/PingMigration.swift apps/macos/ClaudeDashboardTests/Ping/PingMigrationTests.swift apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): PingMigration removes Run Command leftovers"
```

---

### Task 5: Wire auto ping into DashboardViewModel

Replaces the saved-command auto-run in `refreshAll` with auto ping. The Run Command sheet and runner stay until Task 7.

**Files:**
- Create: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+Ping.swift`
- Modify: `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift` (properties ~L134-143, init ~L145-205, reset monitor doc ~L234-239, `shouldRunSavedCommand` ~L253-263, `refreshAll` ~L280-282, ~L331-346, ~L398-414)
- Test: `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift`

**Interfaces:**
- Consumes: `PingController`, `PingTarget` (Task 3); `PingService` (Task 1); `PingLogStore` (Task 2).
- Produces (used by Task 6 views):
  - `DashboardViewModel.pingController: PingController`
  - `DashboardViewModel.autoPingTask: Task<Void, Never>?`
  - `DashboardViewModel.init(..., pingController: PingController? = nil)` (last parameter)
  - `func pingTarget(for account: Account) -> PingTarget?`
  - `func canPing(_ state: AccountUsageState) -> Bool`
  - `func pingNow(_ accountId: UUID) async`
  - `func setAutoPingEnabled(_ on: Bool)`
  - `func runAutoPings(_ targets: [PingTarget])`

- [ ] **Step 1: Write the failing tests**

In `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift`:

1. Delete the whole `// MARK: - shouldRunSavedCommand` block: the doc comment, `makeUsage(fiveHourReset:sevenDayReset:)` and the five `testShouldRunSavedCommand_*` tests (currently lines ~194-235). Check first that `makeUsage` has no other caller: `grep -n "makeUsage" apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift` must list only that block. The rule moved to `PingControllerTests.testWindowNotRunningWhenEitherResetIsNil`.

2. Change `makeViewModelWithStore` to take a pinger and pass a `PingController` with an isolated log and the test defaults:

```swift
    private func makeViewModelWithStore(
        cookies: ChromeCookieResult = ChromeCookieResult(sessionKey: nil, orgId: nil),
        cookieProvider: ((String, Browser) -> ChromeCookieResult)? = nil,
        pinger: Pinging = FakePinger()
    ) throws -> (DashboardViewModel, AccountStore) {
        let detectorFile = tempDir.appendingPathComponent(".claude.json-\(UUID().uuidString)")
        let detector = ClaudeCodeAccountDetector(fileURL: detectorFile)
        let store = AccountStore(defaults: defaults)
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        let api = UsageAPIService(session: URLSession(configuration: config))
        // Isolated SQLite files: a refresh this view model starts can outlive the test,
        // and the default stores point at the user's real databases.
        let pingController = PingController(
            service: pinger,
            logStore: PingLogStore(dbPath: tempDir.appendingPathComponent("pings.sqlite").path),
            defaults: defaults)
        let vm = DashboardViewModel(
            accountStore: store,
            apiService: api,
            logStore: UsageLogStore(dbPath: tempDir.appendingPathComponent("usage.sqlite").path),
            commandLogStore: CommandLogStore(dbPath: tempDir.appendingPathComponent("commands.sqlite").path),
            ccDetector: detector,
            cookieProvider: cookieProvider ?? { _, _ in cookies },
            pingController: pingController)
        return (vm, store)
    }
```

3. Add these tests after `testRefreshBackfillsAccountUuidAndEmailWhenMissing`. `respond(accountBody:)` already answers `/usage` with both `resets_at: null`:

```swift
    // MARK: - Auto ping

    /// Waits for the background auto pings started by `refreshAll`. The view
    /// model's own launch refresh can claim the account first, so this waits on
    /// the pinger rather than on one particular task.
    private func waitForPings(_ pinger: FakePinger, count: Int) async throws {
        for _ in 0..<300 where pinger.calls.count < count {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        await Task.yield()
    }

    private func addIdleAccount(to store: AccountStore, vm: DashboardViewModel) async -> Account {
        let account = makeAccount(orgId: "org-1", accountUuid: "acct-1")
        store.addAccount(account)
        store.saveSessionKey("sk-test", for: account.id)
        respond(accountBody: #"{"uuid":"acct-1","email_address":"person@example.com","memberships":[]}"#)
        await Task.yield()
        XCTAssertEqual(vm.accountStates.count, 1, "sanity: account must reach accountStates before refreshAll runs")
        return account
    }

    func testRefreshAutoPingsAnAccountWhoseWindowIsNotRunning() async throws {
        let pinger = FakePinger()
        let (vm, store) = try makeViewModelWithStore(pinger: pinger)
        _ = await addIdleAccount(to: store, vm: vm)
        vm.pingController.isAutoPingEnabled = true

        await vm.refreshAll()
        try await waitForPings(pinger, count: 1)

        XCTAssertEqual(pinger.calls, ["org-1"])
    }

    func testLaterRefreshesDoNotPingAgainWhileTheApiStillReportsNull() async throws {
        let pinger = FakePinger()
        let (vm, store) = try makeViewModelWithStore(pinger: pinger)
        _ = await addIdleAccount(to: store, vm: vm)
        vm.pingController.isAutoPingEnabled = true

        await vm.refreshAll()
        try await waitForPings(pinger, count: 1)
        await vm.autoPingTask?.value
        await vm.refreshAll()
        await vm.autoPingTask?.value

        XCTAssertEqual(pinger.calls, ["org-1"])
    }

    func testAFailedAutoPingIsNotRetriedByTheRefreshThatFollowsIt() async throws {
        let pinger = FakePinger(result: PingResult(outcome: .blocked, detail: nil))
        let (vm, store) = try makeViewModelWithStore(pinger: pinger)
        _ = await addIdleAccount(to: store, vm: vm)
        vm.pingController.isAutoPingEnabled = true

        await vm.refreshAll()
        try await waitForPings(pinger, count: 1)
        await vm.autoPingTask?.value   // includes the refresh after the ping
        await vm.refreshAll()
        await vm.autoPingTask?.value

        XCTAssertEqual(pinger.calls, ["org-1"])
    }

    func testRefreshDoesNotPingWhenAutoPingIsOff() async throws {
        let pinger = FakePinger()
        let (vm, store) = try makeViewModelWithStore(pinger: pinger)
        _ = await addIdleAccount(to: store, vm: vm)

        await vm.refreshAll()
        await vm.autoPingTask?.value

        XCTAssertTrue(pinger.calls.isEmpty)
        XCTAssertNil(vm.autoPingTask)
    }

    func testManualPingPingsOnceAndRecordsTheResult() async throws {
        let pinger = FakePinger()
        let (vm, store) = try makeViewModelWithStore(pinger: pinger)
        let account = await addIdleAccount(to: store, vm: vm)

        await vm.pingNow(account.id)

        XCTAssertEqual(pinger.calls, ["org-1"])
        XCTAssertEqual(vm.pingController.latest[account.id]?.trigger, .manual)
        XCTAssertEqual(vm.pingController.latest[account.id]?.outcome, .ok)
    }

    func testCanPingOnlyAnActiveAccountWithAKey() async throws {
        let (vm, store) = try makeViewModelWithStore()
        let account = await addIdleAccount(to: store, vm: vm)
        let state = try XCTUnwrap(vm.accountStates.first)
        XCTAssertTrue(vm.canPing(state))

        var expired = account
        expired.status = .expired
        XCTAssertFalse(vm.canPing(AccountUsageState(id: account.id, account: expired)))
    }
```

- [ ] **Step 2: Run to verify failure**

Run:
```bash
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/DashboardViewModelTests 2>&1 | tail -30
```
Expected: build FAILS with "extra argument 'pingController' in call" (or "value of type 'DashboardViewModel' has no member 'pingController'").

- [ ] **Step 3: Add the controller to the view model**

In `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift`:

a) Replace the `pingedAccounts` property and its doc comment (~L136-139):

```swift
    /// Accounts already pinged for the current "circle not started" episode. An account
    /// is cleared once both its windows report a reset again, so the next reset re-fires
    /// the command exactly once instead of on every refresh while `resetsAt` stays nil.
    private var pingedAccounts: Set<UUID> = []
```

with:

```swift
    /// Manual and auto pings; owns the auto-ping latch (see `PingController`).
    let pingController: PingController
    /// The latest batch of auto pings and the refresh after it. Tests await it.
    var autoPingTask: Task<Void, Never>?
```

b) Add the parameter as the last one of `init` (after `autoSwitchSettings: AutoSwitchSettings? = nil`):

```swift
        autoSwitchSettings: AutoSwitchSettings? = nil,
        pingController: PingController? = nil
    ) {
```

c) In the init body, after `self.autoSwitch = AutoSwitchController(settings: switchSettings)`, add:

```swift
        self.pingController = pingController ?? PingController(
            service: PingService(), logStore: PingLogStore(), defaults: AppDefaults.shared)
```

d) After the `autoSwitch.objectWillChange` forwarding block in init, add:

```swift
        self.pingController.objectWillChange
            .sink { [weak self] _ in self?.objectWillChange.send() }
            .store(in: &cancellables)
```

e) In the doc comment of `checkResetTriggers` (~L234-239), replace "so `refreshAll` can fire the saved command to restart the circle within seconds" with "so `refreshAll` can auto-ping to restart the window within seconds", and replace "so the command can never fire twice for one reset" with "so a window is never pinged twice for one reset".

f) Delete the `shouldRunSavedCommand(for:)` function and its doc comment (~L253-263).

g) In `refreshAll`, replace:

```swift
        // Saved commands to run for accounts whose refresh produced no 5h/7d usage.
        var autoCommands: [(accountId: UUID, command: String)] = []
```

with:

```swift
        // Accounts whose fresh usage shows a window not running (see PingController).
        var autoPings: [PingTarget] = []
```

h) In the result loop, replace the block from `// Run the account's saved command once when a window's circle has` through the closing `}` of the `else { pingedAccounts.remove(acctId) }` with:

```swift
                    // Auto ping on this refresh's own fetch, never the stored usage:
                    // a failed fetch (usage == nil) must not ping. A successful fetch
                    // marks the account `.active` below (through the store, which
                    // reaches `accountStates` later), so an `.error` account that just
                    // recovered counts as active now.
                    let pingAccount = accountStates[index].account
                    let pingStatus: AccountStatus = error == nil ? .active : pingAccount.status
                    if let target = pingTarget(for: pingAccount),
                       pingController.claimAutoPing(accountId: accountId, status: pingStatus,
                                                    freshUsage: usage) {
                        autoPings.append(target)
                    }
```

i) Replace the whole block after the task group that starts with `// Fire the de-duplicated saved commands without blocking the refresh spinner.` and ends with the closing `}` of `if !autoCommands.isEmpty { ... }` with:

```swift
        // Fire auto pings without blocking the refresh spinner.
        runAutoPings(autoPings)
```

- [ ] **Step 4: Write the glue extension**

Create `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+Ping.swift`:

```swift
import Foundation

/// Ping glue: building targets, the account page's button, the Auto Ping toggle,
/// and firing auto pings after a refresh. Rules live in `PingController`.
extension DashboardViewModel {

    func pingTarget(for account: Account) -> PingTarget? {
        guard let orgId = account.orgId,
              let sessionKey = accountStore.loadSessionKey(for: account.id) else { return nil }
        return PingTarget(accountId: account.id, orgId: orgId, sessionKey: sessionKey)
    }

    /// The account page's Ping button is enabled for an active account with a key,
    /// while no ping for it is running.
    func canPing(_ state: AccountUsageState) -> Bool {
        state.account.status == .active
            && !pingController.inFlight.contains(state.id)
            && pingTarget(for: state.account) != nil
    }

    /// The Ping button: ping, then refresh that account so its reset time shows.
    func pingNow(_ accountId: UUID) async {
        guard let account = accountStates.first(where: { $0.id == accountId })?.account,
              let target = pingTarget(for: account) else { return }
        if await pingController.ping(target, trigger: .manual) != nil {
            await refreshAll(only: [accountId])
        }
    }

    /// Turning Auto Ping on refreshes now, so idle accounts are pinged without
    /// waiting for the next interval.
    func setAutoPingEnabled(_ on: Bool) {
        pingController.isAutoPingEnabled = on
        if on { Task { await refreshAll() } }
    }

    /// Pings `targets` in parallel in the background, then refreshes them.
    func runAutoPings(_ targets: [PingTarget]) {
        guard !targets.isEmpty else { return }
        let controller = pingController
        autoPingTask = Task {
            await withTaskGroup(of: Void.self) { group in
                for target in targets {
                    group.addTask { _ = await controller.ping(target, trigger: .auto) }
                }
            }
            await refreshAll(only: Set(targets.map(\.accountId)))
        }
    }
}
```

- [ ] **Step 5: Run the view model tests**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/DashboardViewModelTests -only-testing:ClaudeDashboardTests/PingControllerTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`. The six new tests pass; no `shouldRunSavedCommand` tests remain.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel+Ping.swift apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): auto ping replaces the saved-command auto-run"
```

---

### Task 6: Ping UI (account page, Settings, Ping Log)

**Files:**
- Create: `apps/macos/ClaudeDashboard/ViewModels/PingLogViewModel.swift`
- Create: `apps/macos/ClaudeDashboard/Views/PingLogView.swift`
- Delete: `apps/macos/ClaudeDashboard/ViewModels/CommandLogViewModel.swift`, `apps/macos/ClaudeDashboard/Views/CommandLogView.swift`, `apps/macos/ClaudeDashboardTests/CommandLogViewModelTests.swift`
- Modify: `apps/macos/ClaudeDashboard/Views/AccountPane.swift`, `apps/macos/ClaudeDashboard/Views/Settings/GeneralSettingsPane.swift`, `apps/macos/ClaudeDashboard/Models/SidebarItem.swift`, `apps/macos/ClaudeDashboard/Views/MainWindow.swift`
- Test: `apps/macos/ClaudeDashboardTests/Ping/PingLogViewModelTests.swift`, `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift` (sidebar test)

**Interfaces:**
- Consumes: `PingLogStore`, `PingLogEntry` (Task 2); `DashboardViewModel.pingController`, `canPing`, `pingNow`, `setAutoPingEnabled` (Task 5).
- Produces: `SidebarItem.pingLog` (replaces `.commandLog`); `@MainActor final class PingLogViewModel: ObservableObject { init(store: PingLogStore, accountStore: AccountStore, limit: Int = 500); @Published var entries: [PingLogEntry]; func load() async; func clear() async; func accountName(for id: UUID) -> String }`.

- [ ] **Step 1: Write the failing tests**

Create `apps/macos/ClaudeDashboardTests/Ping/PingLogViewModelTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

@MainActor
final class PingLogViewModelTests: XCTestCase {
    private var dbPath: String!
    private var suiteName: String!
    private var store: PingLogStore!
    private var accountStore: AccountStore!

    override func setUp() async throws {
        dbPath = NSTemporaryDirectory() + "ping_log_vm_\(UUID().uuidString).db"
        store = PingLogStore(dbPath: dbPath)
        suiteName = StoreFixture.makeSuiteName()
        accountStore = AccountStore(defaults: UserDefaults(suiteName: suiteName)!)
    }

    override func tearDown() async throws {
        store = nil
        accountStore = nil
        StoreFixture.destroy(suite: suiteName)
        try? FileManager.default.removeItem(atPath: dbPath)
    }

    private func record(_ accountId: UUID) async {
        await store.insert(accountId: accountId, trigger: .manual, startedAt: Date(), finishedAt: Date(),
                           result: PingResult(outcome: .ok, detail: nil))
    }

    func testLoadReturnsLoggedPings() async {
        await record(UUID())
        let vm = PingLogViewModel(store: store, accountStore: accountStore)

        await vm.load()

        XCTAssertEqual(vm.entries.count, 1)
    }

    func testClearEmptiesBothTheListAndTheStore() async {
        await record(UUID())
        let vm = PingLogViewModel(store: store, accountStore: accountStore)
        await vm.load()

        await vm.clear()
        let reloaded = await store.fetch(limit: 10)

        XCTAssertTrue(vm.entries.isEmpty)
        XCTAssertTrue(reloaded.isEmpty)
    }

    func testAccountNameForALiveAndARemovedAccount() async {
        let account = Account(id: UUID(), name: "Work", email: nil, chromeProfilePath: "Profile 1",
                              chromeProfileName: nil, orgId: "org-1", plan: .pro, lastSynced: nil,
                              status: .active, source: .browser)
        accountStore.addAccount(account)
        let vm = PingLogViewModel(store: store, accountStore: accountStore)

        XCTAssertEqual(vm.accountName(for: account.id), "Work")
        XCTAssertEqual(vm.accountName(for: UUID()), "Removed account")
    }
}
```

The `Account` initializer above matches the one `AccountStoreTests` uses (`id, name, email, chromeProfilePath, chromeProfileName, orgId, plan, lastSynced, status, source`).

In `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift`, in `testSidebarSectionsListEveryStaticRowExactlyOnce`, replace both `.commandLog` occurrences with `.pingLog`.

Delete `apps/macos/ClaudeDashboardTests/CommandLogViewModelTests.swift`.

- [ ] **Step 2: Regenerate and run to verify failure**

Run:
```bash
git rm -q apps/macos/ClaudeDashboardTests/CommandLogViewModelTests.swift
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingLogViewModelTests 2>&1 | tail -30
```
Expected: build FAILS with "cannot find 'PingLogViewModel' in scope" and "type 'SidebarItem' has no member 'pingLog'".

- [ ] **Step 3: Ping Log view model and view**

Create `apps/macos/ClaudeDashboard/ViewModels/PingLogViewModel.swift`:

```swift
import Foundation
import Combine

/// State of the Ping Log pane.
///
///     let vm = PingLogViewModel(store: dashboard.pingController.logStore,
///                               accountStore: dashboard.accountStore)
///     await vm.load()
@MainActor
final class PingLogViewModel: ObservableObject {
    @Published var entries: [PingLogEntry] = []

    private let store: PingLogStore
    private let accountStore: AccountStore
    private let limit: Int

    init(store: PingLogStore, accountStore: AccountStore, limit: Int = 500) {
        self.store = store
        self.accountStore = accountStore
        self.limit = limit
    }

    func load() async {
        entries = await store.fetch(limit: limit)
    }

    func clear() async {
        await store.clear()
        entries = []
    }

    /// "Removed account" once the account is gone; its rows stay until they age out.
    func accountName(for id: UUID) -> String {
        accountStore.accounts.first(where: { $0.id == id })?.name ?? "Removed account"
    }
}
```

Create `apps/macos/ClaudeDashboard/Views/PingLogView.swift`:

```swift
import SwiftUI

/// Tools > Ping Log: every ping, newest first.
///
///     PingLogView(viewModel: pingLogViewModel)
struct PingLogView: View {
    @ObservedObject var viewModel: PingLogViewModel
    @State private var confirmingClear = false

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .medium
        return f
    }()

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if viewModel.entries.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(viewModel.entries) { entry in
                            row(entry)
                            Divider()
                        }
                    }
                }
            }
        }
        .task { await viewModel.load() }
    }

    private var header: some View {
        PaneHeader(title: "Ping Log") {
            Button(action: { Task { await viewModel.load() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .help("Reload the ping log")
            Button(role: .destructive, action: { confirmingClear = true }) {
                Label("Clear All", systemImage: "trash")
            }
            .disabled(viewModel.entries.isEmpty)
            .help("Delete every ping log entry")
        }
        .confirmationDialog("Clear the ping log?", isPresented: $confirmingClear, titleVisibility: .visible) {
            Button("Clear All", role: .destructive) { Task { await viewModel.clear() } }
            Button("Cancel", role: .cancel) {}
        }
    }

    private func row(_ entry: PingLogEntry) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(Self.dateFormatter.string(from: entry.startedAt))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    triggerBadge(entry.trigger)
                    Text(viewModel.accountName(for: entry.accountId))
                        .font(.caption.weight(.medium))
                }
                if let detail = entry.detail {
                    Text(detail)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            Spacer(minLength: 0)
            VStack(alignment: .trailing, spacing: 4) {
                Text(entry.outcome.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(entry.outcome == .ok ? Color.green : Color.red)
                Text(String(format: "%.1fs", max(0, entry.finishedAt.timeIntervalSince(entry.startedAt))))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private func triggerBadge(_ trigger: PingTrigger) -> some View {
        let color: Color = trigger == .manual ? .blue : .purple
        return Text(trigger.label)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.18))
            .foregroundStyle(color)
            .clipShape(Capsule())
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "dot.radiowaves.left.and.right")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("No pings yet.")
                .foregroundStyle(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
```

Delete the old pane:
```bash
git rm -q apps/macos/ClaudeDashboard/ViewModels/CommandLogViewModel.swift apps/macos/ClaudeDashboard/Views/CommandLogView.swift
```

- [ ] **Step 4: Sidebar and main window**

In `apps/macos/ClaudeDashboard/Models/SidebarItem.swift`:
- `case commandLog` becomes `case pingLog`.
- `case .commandLog: return Style(title: "Command Log", systemImage: "list.bullet.rectangle", tileColor: .indigo)` becomes `case .pingLog: return Style(title: "Ping Log", systemImage: "dot.radiowaves.left.and.right", tileColor: .indigo)`.
- `static let toolItems: [SidebarItem] = [.commandLog, .help]` becomes `[.pingLog, .help]`.

In `apps/macos/ClaudeDashboard/Views/MainWindow.swift`:
- `@StateObject private var commandLogViewModel: CommandLogViewModel` becomes `@StateObject private var pingLogViewModel: PingLogViewModel`.
- In `init`, replace the `_commandLogViewModel = StateObject(...)` statement with:

```swift
        _pingLogViewModel = StateObject(wrappedValue: PingLogViewModel(
            store: viewModel.pingController.logStore,
            accountStore: viewModel.accountStore
        ))
```

- In `pane`, replace `case .commandLog: CommandLogView(viewModel: commandLogViewModel)` with:

```swift
        case .pingLog:
            PingLogView(viewModel: pingLogViewModel)
```

Then search for any other `.commandLog` use: `grep -rn "commandLog" apps/macos/ClaudeDashboard` must print only `commandLogStore` lines in `DashboardViewModel.swift` (removed in Task 7).

- [ ] **Step 5: Ping row on the account page**

In `apps/macos/ClaudeDashboard/Views/AccountPane.swift`, replace:

```swift
            Divider()
            actionRow("Run a command for this account") {
                Button("Run Command\u{2026}", action: onRunCommand)
                    .help("Run a saved command for this account")
            }
```

with:

```swift
            Divider()
            actionRow("Start the 5h window now") {
                VStack(alignment: .trailing, spacing: 2) {
                    HStack(spacing: 6) {
                        if dashboardViewModel.pingController.inFlight.contains(state.id) {
                            ProgressView().controlSize(.small)
                        }
                        Button("Ping") {
                            Task { await dashboardViewModel.pingNow(state.id) }
                        }
                        .disabled(!dashboardViewModel.canPing(state))
                        .help("Send one short message from this account to start its 5h and 7d windows")
                    }
                    if let entry = dashboardViewModel.pingController.latest[state.id] {
                        Text(entry.caption(time: { $0.formatted(date: .omitted, time: .shortened) }))
                            .font(.caption)
                            .foregroundStyle(entry.outcome == .ok ? Color.secondary : Color.red)
                    }
                }
            }
```

On the outer `VStack(spacing: 0)` in `body`, after `.claudeCodeSwitchAlert(dashboardViewModel)`, add:

```swift
        .task(id: state.id) { await dashboardViewModel.pingController.loadLatest(for: state.id) }
```

The `onRunCommand` parameter stays until Task 7 (MainWindow still passes it).

- [ ] **Step 6: Auto Ping in Settings**

In `apps/macos/ClaudeDashboard/Views/Settings/GeneralSettingsPane.swift`, insert after the `Section("Cards") { ... }` block (it is removed in Task 7):

```swift
                Section {
                    Toggle("Ping when a usage window is not running", isOn: Binding(
                        get: { viewModel.pingController.isAutoPingEnabled },
                        set: { viewModel.setAutoPingEnabled($0) }))
                } header: {
                    Text("Auto Ping")
                } footer: {
                    Text("Starts a new window for every active account whose 5h or 7d window is not running. Each ping sends one short message from that account.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
```

- [ ] **Step 7: Regenerate, build and run the tests**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/PingLogViewModelTests -only-testing:ClaudeDashboardTests/DashboardViewModelTests 2>&1 | tail -30
```
Expected: `** TEST SUCCEEDED **`.

- [ ] **Step 8: Commit**

```bash
git add -A apps/macos/ClaudeDashboard apps/macos/ClaudeDashboardTests apps/macos/ClaudeDashboard.xcodeproj/project.pbxproj
git commit -m "feat(macos): Ping button, Auto Ping setting and Ping Log pane"
```

---

### Task 7: Remove Run Command and run the migration at launch

**Files:**
- Delete: `apps/macos/ClaudeDashboard/Services/CommandRunner.swift`, `Services/CommandClassifier.swift`, `Services/TerminalLauncher.swift`, `Services/ProcessTree.swift`, `Services/RunningProcessRegistry.swift`, `Services/CommandLogStore.swift`, `Models/CommandLogModels.swift`, `Models/RunCommandSettings.swift`, `Views/RunCommandSheet.swift`
- Delete tests: `ClaudeDashboardTests/CommandRunnerTests.swift`, `CommandClassifierTests.swift`, `TerminalLauncherTests.swift`, `ProcessTreeTests.swift`, `RunningProcessRegistryTests.swift`, `CommandLogStoreTests.swift`, `CommandLogModelsTests.swift`
- Modify: `DashboardViewModel.swift`, `AccountPane.swift`, `AccountCard.swift`, `DashboardPane.swift`, `MenuBarPopover.swift`, `MainWindow.swift`, `Settings/GeneralSettingsPane.swift`, `ClaudeDashboardApp.swift`, `Services/AccountStore.swift`, `Views/HelpView.swift`
- Test: `ClaudeDashboardTests/AccountStoreTests.swift`, `ClaudeDashboardTests/DashboardViewModelTests.swift`

**Interfaces:**
- Consumes: `PingMigration.run` (Task 4), `PingLogStore.defaultDirectory()` (Task 2).
- Produces: `AccountPane(dashboardViewModel:state:)` and `DashboardPane(viewModel:onAddAccount:)` without the `onRunCommand` parameter; `AccountCard` without `onRunCommand`.

- [ ] **Step 1: Delete the subsystem and its tests**

```bash
cd apps/macos
git rm -q ClaudeDashboard/Services/CommandRunner.swift ClaudeDashboard/Services/CommandClassifier.swift \
  ClaudeDashboard/Services/TerminalLauncher.swift ClaudeDashboard/Services/ProcessTree.swift \
  ClaudeDashboard/Services/RunningProcessRegistry.swift ClaudeDashboard/Services/CommandLogStore.swift \
  ClaudeDashboard/Models/CommandLogModels.swift ClaudeDashboard/Models/RunCommandSettings.swift \
  ClaudeDashboard/Views/RunCommandSheet.swift \
  ClaudeDashboardTests/CommandRunnerTests.swift ClaudeDashboardTests/CommandClassifierTests.swift \
  ClaudeDashboardTests/TerminalLauncherTests.swift ClaudeDashboardTests/ProcessTreeTests.swift \
  ClaudeDashboardTests/RunningProcessRegistryTests.swift ClaudeDashboardTests/CommandLogStoreTests.swift \
  ClaudeDashboardTests/CommandLogModelsTests.swift
xcodegen generate
cd ../..
```

- [ ] **Step 2: DashboardViewModel**

In `apps/macos/ClaudeDashboard/ViewModels/DashboardViewModel.swift`:
- Delete the properties `let commandLogStore: CommandLogStore` and `let commandRunner: CommandRunner`.
- Delete the init parameters `commandLogStore: CommandLogStore? = nil,` and `commandRunner: CommandRunner? = nil,`.
- Delete the three init lines:

```swift
        let cmdStore = commandLogStore ?? CommandLogStore()
        self.commandLogStore = cmdStore
        self.commandRunner = commandRunner ?? CommandRunner(store: cmdStore)
```

In `apps/macos/ClaudeDashboardTests/DashboardViewModelTests.swift`, in `makeViewModelWithStore`, delete the line `commandLogStore: CommandLogStore(dbPath: tempDir.appendingPathComponent("commands.sqlite").path),`.

- [ ] **Step 3: Account page, main window, dashboard pane**

`apps/macos/ClaudeDashboard/Views/AccountPane.swift`:
- Doc comment example becomes:

```swift
///     AccountPane(dashboardViewModel: vm, state: state)
///         .id(state.id)
```

- Delete `let onRunCommand: () -> Void` and `self.onRunCommand = onRunCommand`; the init signature becomes `init(dashboardViewModel: DashboardViewModel, state: AccountUsageState)`.

`apps/macos/ClaudeDashboard/Views/MainWindow.swift`:
- Doc comment line `/// Sheets (Setup, Run Command) hang off this view so any pane can open them.` becomes `/// The Setup sheet hangs off this view so any pane can open it.`
- Delete `@State private var runCommandAccount: Account?` and the whole `.sheet(item: $runCommandAccount) { ... }` modifier.
- `DashboardPane(viewModel: viewModel, onAddAccount: { showingSetup = true }, onRunCommand: { runCommandAccount = $0 })` becomes `DashboardPane(viewModel: viewModel, onAddAccount: { showingSetup = true })`.
- `AccountPane(dashboardViewModel: viewModel, state: state, onRunCommand: { runCommandAccount = state.account })` becomes `AccountPane(dashboardViewModel: viewModel, state: state)`.

`apps/macos/ClaudeDashboard/Views/DashboardPane.swift`:
- Doc comment example becomes `///     DashboardPane(viewModel: vm, onAddAccount: { showingSetup = true })`.
- Delete `let onRunCommand: (Account) -> Void`, the `/// Same setting as the menu bar popover...` comment, and the `@AppStorage(MenuBarPopover.showRunCommandKey, ...)` / `private var showRunCommand = false` pair.
- Delete the `onRunCommand: showRunCommand ? { onRunCommand(state.account) } : nil,` argument.

- [ ] **Step 4: Card and menu bar popover**

`apps/macos/ClaudeDashboard/Views/AccountCard.swift`:
- Delete `/// nil hides the Run Command button.` and `var onRunCommand: (() -> Void)? = nil`.
- `/// The header's icon buttons (Switch, Run Command): hover background, tight padding.` becomes `/// The header's icon buttons (priority tick, Switch): hover background, tight padding.`
- Delete the whole `if let onRunCommand { Button { ... } ... .help("Run a saved command for this account") }` block.

`apps/macos/ClaudeDashboard/Views/MenuBarPopover.swift`:
- Delete `@State private var runCommandAccount: Account? = nil`.
- Delete the comment `/// Off by default: the card's Run Command button ...` (two lines), the `@AppStorage(MenuBarPopover.showRunCommandKey, ...)` / `private var showRunCommand = false` pair and `static let showRunCommandKey = "menuBarShowRunCommand"`.
- Delete the `onRunCommand: showRunCommand ? { runCommandAccount = state.account } : nil,` argument.
- Delete the whole `.overlay { if let account = runCommandAccount { ZStack { ... RunCommandSheet(...) ... } } }` modifier (the first `.overlay` after `.fixedSize(horizontal: false, vertical: true)`); keep `.claudeCodeSwitchOverlay(viewModel)` and the quit `InlinePrompt` overlay.

- [ ] **Step 5: Settings, Help**

`apps/macos/ClaudeDashboard/Views/Settings/GeneralSettingsPane.swift`:
- Doc comment: replace "the cards' Run Command button (menu bar and Dashboard)," with "Auto Ping (on/off),".
- Delete the `@AppStorage(MenuBarPopover.showRunCommandKey, ...)` / `private var showRunCommandOnCards = false` pair.
- Delete `Section("Cards") { Toggle("Show Run Command button on cards", isOn: $showRunCommandOnCards) }`.

`apps/macos/ClaudeDashboard/Views/HelpView.swift`: replace

```swift
            bodyText("The terminal icon on a card opens **Run Command** — enter any shell command and it runs using that account's context, then the dashboard refreshes. Turn the icon on or off in **Settings > General > Cards**.")
```

with:

```swift
            bodyText("On an account's page, **Ping** sends one short message from that account to start its 5h and 7d windows now. Turn on **Settings › General › Auto Ping** to ping every active account whose window is not running after each refresh. Every ping is listed in **Ping Log**.")
```

- [ ] **Step 6: App delegate, launch migration**

`apps/macos/ClaudeDashboard/ClaudeDashboardApp.swift`:
- Delete `appDelegate.runningProcesses = viewModel.commandRunner.registry` in `.onAppear`.
- Delete `var runningProcesses: RunningProcessRegistry?` in `AppDelegate`.
- Delete the whole `applicationWillTerminate(_:)` method (it only reaped command processes).
- In `windowShouldClose`, the comment `// Dismiss any presented sheets (Setup, Run Command) so AppKit` becomes `// Dismiss any presented sheet (Setup) so AppKit`.
- In `openDashboardWindow`, the comment lines

```swift
        // ending its sheets or swapping its content would dismiss a Run Command
        // sheet, whose onDisappear cancels the run and kills the process tree.
```

become:

```swift
        // ending its sheets or swapping its content would dismiss an open Setup
        // sheet mid-scan.
```

- In `struct ClaudeDashboardApp`, after the `@NSApplicationDelegateAdaptor` line, add:

```swift
    init() {
        // The test host is the real app; its launch must not delete files on disk.
        if !AppDefaults.isRunningTests() {
            PingMigration.run(defaults: AppDefaults.shared, logsDirectory: PingLogStore.defaultDirectory())
        }
    }
```

- [ ] **Step 7: AccountStore and its tests**

`apps/macos/ClaudeDashboard/Services/AccountStore.swift`:
- In `init`, delete the comment block starting `// Only a store that actually decoded says which accounts are live.` and the `if loaded.quarantined == nil { RunCommandSettings.prune(...) }` statement. `loaded` is still used for `self.accounts`.
- In `removeAccount(id:)`, delete the two comment lines and `RunCommandSettings.remove(for: id, in: defaults)`.

`apps/macos/ClaudeDashboardTests/AccountStoreTests.swift`: delete the `// MARK: - Saved run commands` section with `testRemovingAnAccountDropsItsSavedCommand`, `testLoadingTheStorePrunesCommandsOfAccountsThatAreGone` and `testUnreadableStoreDoesNotPruneAnyCommand` (and their doc comments). The quarantine behaviour itself stays covered by `testUnparseableStoreIsKeptAsideRatherThanOverwritten`.

- [ ] **Step 8: Prove nothing references the old feature**

Run:
```bash
rtk proxy grep -rnE "CommandRunner|RunningProcessRegistry|TerminalLauncher|ProcessTree|CommandClassifier|CommandLog|RunCommand|runCommand|showRunCommand|commandRunner|CommandTrigger|CommandStatus|shouldRunSavedCommand|pingedAccounts" apps/macos/ClaudeDashboard apps/macos/ClaudeDashboardTests apps/macos/Shared apps/macos/Helper apps/macos/HelperTests
```
Expected: matches only in `Services/Ping/PingMigration.swift` (`runCommand_`, `runCommandTerminal_`, `menuBarShowRunCommand`) and `ClaudeDashboardTests/Ping/PingMigrationTests.swift`.

- [ ] **Step 9: Run both test schemes**

Run:
```bash
(cd apps/macos && xcodegen generate)
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests 2>&1 | tail -30
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardHelperTests 2>&1 | tail -15
```
Expected: `** TEST SUCCEEDED **` for both. These run for several minutes: start them in the background with output redirected to a log in the scratchpad, and check the log every ~30 s.

- [ ] **Step 10: Commit**

```bash
git add -A apps/macos
git commit -m "feat(macos): remove Run Command; clean its leftovers at launch"
```

---

### Task 8: Documentation and real-app check

**Files:**
- Modify: `CLAUDE.md`, `contract/README.md`

- [ ] **Step 1: CLAUDE.md**

In the **Services Layer** list, after the **AutoSwitch** bullet, add:

```markdown
- **Ping** (`Shared/PingService.swift`, `Services/Ping/`) — starts an account's 5h/7d window by
  sending one short claude.ai message with its session key (create conversation, completion,
  delete; see `contract/README.md`, "Ping"). The account page's **Ping** button pings by hand;
  Settings > General > **Auto Ping** (key `autoPingEnabled`, default off, one switch for every
  account) makes `PingController` ping each `.active` account whose fresh usage shows
  `five_hour` or `seven_day` `resets_at: null`, latched until both windows run again, with a
  10-minute retry after a failed auto ping. `PingLogStore` keeps the newest 500 pings in
  `ping_logs.db` (Tools > Ping Log). `PingMigration` deletes Run Command leftovers
  (`command_logs.db`, `runCommand_*` keys) at launch. macOS only: Windows keeps Run Command.
  Spec: `docs/superpowers/specs/2026-10-14-ping-design.md`.
```

In the **MainWindow** bullet, replace `Tools (Command Log, Help)` with `Tools (Ping Log, Help)`.

- [ ] **Step 2: contract/README.md**

Append at the end:

```markdown
## Ping

Starting an account's usage windows without Claude Code: one short claude.ai message sent
with the account's `sessionKey` cookie and the usual `anthropic-client-platform: web_claude_ai`
header. Implemented on macOS only (`apps/macos/Shared/PingService.swift`). The endpoints are
undocumented claude.ai internals and may change; every outcome is logged so a change shows up.

1. `POST /api/organizations/{orgId}/chat_conversations` with `{"uuid": <new lowercase UUID>, "name": ""}`.
2. `POST /api/organizations/{orgId}/chat_conversations/{uuid}/completion` with
   `{"prompt": "Reply with just: ok", "timezone": <IANA zone>, "attachments": [], "files": [],
   "rendering_mode": "messages"}`. The reply is SSE; an `event: message_start` means the
   message was accepted and the windows started. No `model` field: the account default is used.
3. `DELETE /api/organizations/{orgId}/chat_conversations/{uuid}`, best effort, also after a
   failed step 2.

| Response | Outcome |
|---|---|
| `message_start` seen | ok (a failed DELETE only adds a note) |
| 401, or 403 with a JSON body | expired |
| 403 with a `cf-mitigated` header or an HTML body | blocked |
| anything else (429, timeout, network error, no `message_start`) | failed |

Observed 2026-10-14: before a ping `five_hour.resets_at` was `null`; right after, it was the
ping time plus 5 h rounded down to a 10-minute boundary, while utilization stayed 0.
```

- [ ] **Step 3: Commit the docs**

```bash
git add CLAUDE.md contract/README.md docs/superpowers/specs/2026-10-14-ping-design.md
git commit -m "docs: ping replaces run command on macOS"
```

(`docs/superpowers/` is gitignored; the spec is already tracked, so `git add` updates it. If git refuses, use `git add -f` for that path, as the spec commit did.)

- [ ] **Step 4: Real-app check (ask the user before this step)**

This touches the user's real accounts and their real `UserDefaults` domain (the Debug build shares the bundle id), and each ping uses one message of quota. Ask first.

```bash
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Quit the installed app, launch the built one from DerivedData, then:
1. Open an account whose 5h window is not running (`resets_at: null`), click **Ping**: the spinner shows, then the caption "Pinged at HH:MM", and the gauge shows a reset time.
2. Tools > **Ping Log** lists one Manual row with outcome OK.
3. Settings > General > **Auto Ping** on: every other active account with a window not running gets exactly one Auto row; a second manual Refresh adds none.
4. Cards (Dashboard and menu bar) have no terminal icon; Settings has no "Cards" section.
5. `ls ~/Library/Application\ Support/ClaudeDashboard/` shows `ping_logs.db` and no `command_logs.db`.

Report what was seen, with any failure verbatim.
```

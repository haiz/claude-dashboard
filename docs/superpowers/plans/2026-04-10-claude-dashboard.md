# Claude Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native macOS menubar + window app that displays Claude usage (5h and 7d limits with reset countdowns) across multiple accounts, syncing auth from Chrome profiles automatically.

**Architecture:** MVVM with service layer. ChromeCookieService reads/decrypts Chrome cookies → cached in Keychain via KeychainService → UsageAPIService fetches usage from claude.ai → DashboardViewModel drives SwiftUI views (MenuBarExtra popover + NSWindow).

**Tech Stack:** Swift 6.3, SwiftUI, macOS 13+, Security framework, CommonCrypto, SQLite3. Zero external dependencies. Xcode project generated via xcodegen.

**Spec:** `docs/superpowers/specs/2026-04-10-claude-dashboard-design.md`

---

## File Map

| File | Responsibility |
|------|---------------|
| `project.yml` | xcodegen project spec |
| `ClaudeDashboard/Info.plist` | LSUIElement=true, bundle metadata |
| `ClaudeDashboard/ClaudeDashboardApp.swift` | @main entry, MenuBarExtra + Window setup |
| `ClaudeDashboard/Models/Account.swift` | Account, AccountPlan, AccountStatus |
| `ClaudeDashboard/Models/UsageData.swift` | UsageData, UsageLimit (API response) |
| `ClaudeDashboard/Services/KeychainService.swift` | Keychain CRUD wrapper |
| `ClaudeDashboard/Services/AccountStore.swift` | Account list persistence (UserDefaults + Keychain) |
| `ClaudeDashboard/Services/ChromeCookieService.swift` | Chrome profile scanning + AES cookie decryption |
| `ClaudeDashboard/Services/UsageAPIService.swift` | claude.ai usage API client |
| `ClaudeDashboard/ViewModels/DashboardViewModel.swift` | Orchestrates refresh, holds UI state, defines AccountUsageState |
| `ClaudeDashboard/Views/UsageBar.swift` | Single progress bar with gradient color |
| `ClaudeDashboard/Views/AccountCard.swift` | Card displaying one account's usage |
| `ClaudeDashboard/Views/MenuBarPopover.swift` | Popover content (scrollable card list + toolbar) |
| `ClaudeDashboard/Views/DashboardWindow.swift` | Full window view (grid + settings tab) |
| `ClaudeDashboard/Views/SettingsView.swift` | Account management (rename, re-sync, remove) |
| `ClaudeDashboard/Views/SetupView.swift` | First-time Chrome profile picker |
| `ClaudeDashboardTests/UsageDataTests.swift` | JSON decoding tests |
| `ClaudeDashboardTests/AccountStoreTests.swift` | Account persistence tests |
| `ClaudeDashboardTests/ChromeCookieServiceTests.swift` | Cookie decryption tests |
| `ClaudeDashboardTests/UsageAPIServiceTests.swift` | API client tests with URLProtocol mock |

---

### Task 1: Project Scaffolding

**Files:**
- Create: `project.yml`
- Create: `ClaudeDashboard/Info.plist`
- Create: `ClaudeDashboard/Assets.xcassets/Contents.json`
- Create: `ClaudeDashboard/Assets.xcassets/AppIcon.appiconset/Contents.json`
- Create: `ClaudeDashboard/ClaudeDashboardApp.swift`
- Create: `.gitignore`

- [ ] **Step 1: Install xcodegen**

```bash
brew install xcodegen
```

Expected: `xcodegen` available in PATH.

- [ ] **Step 2: Create .gitignore**

Create `.gitignore`:

```
# Xcode
*.xcodeproj/xcuserdata/
*.xcworkspace/xcuserdata/
DerivedData/
build/
*.pbxuser
*.mode1v3
*.mode2v3
*.perspectivev3
*.moved-aside
*.hmap
*.ipa
*.dSYM.zip
*.dSYM
xcuserdata/

# macOS
.DS_Store

# Swift Package Manager
.build/
.swiftpm/
```

- [ ] **Step 3: Create project.yml (xcodegen spec)**

Create `project.yml`:

```yaml
name: ClaudeDashboard
options:
  bundleIdPrefix: com.claude-dashboard
  deploymentTarget:
    macOS: "13.0"
  xcodeVersion: "16.3"
  minimumXcodeGenVersion: "2.40"
settings:
  base:
    SWIFT_VERSION: "5.0"
    MACOSX_DEPLOYMENT_TARGET: "13.0"
targets:
  ClaudeDashboard:
    type: application
    platform: macOS
    sources:
      - ClaudeDashboard
    settings:
      base:
        INFOPLIST_FILE: ClaudeDashboard/Info.plist
        GENERATE_INFOPLIST_FILE: NO
        CODE_SIGN_IDENTITY: "-"
        CODE_SIGN_STYLE: Manual
        PRODUCT_BUNDLE_IDENTIFIER: com.claude-dashboard.app
        ENABLE_APP_SANDBOX: NO
        ENABLE_HARDENED_RUNTIME: NO
    info:
      path: ClaudeDashboard/Info.plist
      properties: {}
  ClaudeDashboardTests:
    type: bundle.unit-test
    platform: macOS
    sources:
      - ClaudeDashboardTests
    dependencies:
      - target: ClaudeDashboard
    settings:
      base:
        TEST_HOST: "$(BUILT_PRODUCTS_DIR)/ClaudeDashboard.app/Contents/MacOS/ClaudeDashboard"
        BUNDLE_LOADER: "$(TEST_HOST)"
```

- [ ] **Step 4: Create Info.plist**

Create `ClaudeDashboard/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Claude Dashboard</string>
    <key>CFBundleDisplayName</key>
    <string>Claude Dashboard</string>
    <key>CFBundleIdentifier</key>
    <string>$(PRODUCT_BUNDLE_IDENTIFIER)</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>$(EXECUTABLE_NAME)</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>$(MACOSX_DEPLOYMENT_TARGET)</string>
</dict>
</plist>
```

- [ ] **Step 5: Create Asset Catalog**

Create `ClaudeDashboard/Assets.xcassets/Contents.json`:

```json
{
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}
```

Create `ClaudeDashboard/Assets.xcassets/AppIcon.appiconset/Contents.json`:

```json
{
  "images" : [
    {
      "idiom" : "mac",
      "scale" : "1x",
      "size" : "16x16"
    },
    {
      "idiom" : "mac",
      "scale" : "2x",
      "size" : "16x16"
    },
    {
      "idiom" : "mac",
      "scale" : "1x",
      "size" : "32x32"
    },
    {
      "idiom" : "mac",
      "scale" : "2x",
      "size" : "32x32"
    },
    {
      "idiom" : "mac",
      "scale" : "1x",
      "size" : "128x128"
    },
    {
      "idiom" : "mac",
      "scale" : "2x",
      "size" : "128x128"
    },
    {
      "idiom" : "mac",
      "scale" : "1x",
      "size" : "256x256"
    },
    {
      "idiom" : "mac",
      "scale" : "2x",
      "size" : "256x256"
    },
    {
      "idiom" : "mac",
      "scale" : "1x",
      "size" : "512x512"
    },
    {
      "idiom" : "mac",
      "scale" : "2x",
      "size" : "512x512"
    }
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}
```

- [ ] **Step 6: Create minimal App entry point**

Create `ClaudeDashboard/ClaudeDashboardApp.swift`:

```swift
import SwiftUI

@main
struct ClaudeDashboardApp: App {
    var body: some Scene {
        MenuBarExtra("Claude Dashboard", systemImage: "chart.bar.fill") {
            Text("Claude Dashboard")
                .padding()
        }
        .menuBarExtraStyle(.window)
    }
}
```

- [ ] **Step 7: Create empty test file so target compiles**

Create `ClaudeDashboardTests/ClaudeDashboardTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class ClaudeDashboardTests: XCTestCase {
    func testPlaceholder() {
        XCTAssertTrue(true)
    }
}
```

- [ ] **Step 8: Generate Xcode project and verify build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate
```

Expected: `Generated ClaudeDashboard.xcodeproj`

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard -configuration Debug build 2>&1 | tail -5
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 9: Run tests to verify test target works**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -configuration Debug test 2>&1 | tail -10
```

Expected: `Test Suite 'All tests' passed`

- [ ] **Step 10: Initialize git and commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git init && git add -A && git commit -m "feat: scaffold ClaudeDashboard Xcode project"
```

---

### Task 2: Data Models + JSON Decoding

**Files:**
- Create: `ClaudeDashboard/Models/UsageData.swift`
- Create: `ClaudeDashboard/Models/Account.swift`
- Modify: `ClaudeDashboardTests/ClaudeDashboardTests.swift` → rename to `ClaudeDashboardTests/UsageDataTests.swift`

- [ ] **Step 1: Write failing test for UsageData JSON decoding**

Create `ClaudeDashboardTests/UsageDataTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class UsageDataTests: XCTestCase {

    func testDecodesFullResponse() throws {
        let json = """
        {
          "five_hour": { "utilization": 42.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 18.0, "resets_at": "2026-04-14T16:59:59.661657+00:00" },
          "seven_day_sonnet": { "utilization": 0.0, "resets_at": null },
          "seven_day_opus": null,
          "seven_day_oauth_apps": null,
          "seven_day_cowork": null,
          "extra_usage": null
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertEqual(usage.fiveHour.utilization, 42.0)
        XCTAssertNotNil(usage.fiveHour.resetsAt)
        XCTAssertEqual(usage.sevenDay.utilization, 18.0)
        XCTAssertNotNil(usage.sevenDay.resetsAt)
    }

    func testDecodesResponseWithNullResetsAt() throws {
        let json = """
        {
          "five_hour": { "utilization": 0.0, "resets_at": null },
          "seven_day": { "utilization": 0.0, "resets_at": null }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertEqual(usage.fiveHour.utilization, 0.0)
        XCTAssertNil(usage.fiveHour.resetsAt)
        XCTAssertEqual(usage.sevenDay.utilization, 0.0)
        XCTAssertNil(usage.sevenDay.resetsAt)
    }

    func testDecodesDateWithFractionalSeconds() throws {
        let json = """
        {
          "five_hour": { "utilization": 10.0, "resets_at": "2026-04-10T18:59:59.661633+00:00" },
          "seven_day": { "utilization": 5.0, "resets_at": "2026-04-14T16:59:59+00:00" }
        }
        """.data(using: .utf8)!

        let usage = try UsageData.decode(from: json)

        XCTAssertNotNil(usage.fiveHour.resetsAt)
        XCTAssertNotNil(usage.sevenDay.resetsAt)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(error:|FAIL|PASS|Build)"
```

Expected: compilation error — `UsageData` not found.

- [ ] **Step 3: Implement UsageData and UsageLimit**

Create `ClaudeDashboard/Models/UsageData.swift`:

```swift
import Foundation

struct UsageLimit: Codable, Equatable {
    let utilization: Double
    let resetsAt: Date?

    enum CodingKeys: String, CodingKey {
        case utilization
        case resetsAt = "resets_at"
    }
}

struct UsageData: Codable, Equatable {
    let fiveHour: UsageLimit
    let sevenDay: UsageLimit

    enum CodingKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
    }

    static func decode(from data: Data) throws -> UsageData {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let dateString = try container.decode(String.self)

            let formatterWithFraction = ISO8601DateFormatter()
            formatterWithFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = formatterWithFraction.date(from: dateString) {
                return date
            }

            let formatterBasic = ISO8601DateFormatter()
            formatterBasic.formatOptions = [.withInternetDateTime]
            if let date = formatterBasic.date(from: dateString) {
                return date
            }

            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Cannot decode date: \(dateString)"
            )
        }
        return try decoder.decode(UsageData.self, from: data)
    }
}
```

- [ ] **Step 4: Implement Account model**

Create `ClaudeDashboard/Models/Account.swift`:

```swift
import Foundation

enum AccountPlan: String, Codable, CaseIterable {
    case pro = "Pro"
    case max200 = "Max"
}

enum AccountStatus: String, Codable {
    case active
    case expired
    case error
}

struct Account: Identifiable, Codable, Equatable {
    let id: UUID
    var name: String
    var chromeProfilePath: String
    var orgId: String?
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus

    var isConfigured: Bool {
        orgId != nil
    }
}
```

- [ ] **Step 5: Delete placeholder test file**

Delete `ClaudeDashboardTests/ClaudeDashboardTests.swift` (replaced by `UsageDataTests.swift`).

- [ ] **Step 6: Regenerate project and run tests**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Test Suite|Executed|FAIL)"
```

Expected: `Executed 3 tests, with 0 failures`

- [ ] **Step 7: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add UsageData and Account models with JSON decoding"
```

---

### Task 3: KeychainService

**Files:**
- Create: `ClaudeDashboard/Services/KeychainService.swift`

- [ ] **Step 1: Implement KeychainService**

Create `ClaudeDashboard/Services/KeychainService.swift`:

```swift
import Foundation
import Security

enum KeychainService {
    private static let servicePrefix = "com.claude-dashboard"
    private static var cache: [String: String] = [:]

    static func save(key: String, value: String) {
        cache[key] = value

        let data = value.data(using: .utf8)!
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
        ]

        SecItemDelete(query as CFDictionary)

        var addQuery = query
        addQuery[kSecValueData as String] = data
        addQuery[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock

        SecItemAdd(addQuery as CFDictionary, nil)
    }

    static func load(key: String) -> String? {
        if let cached = cache[key] {
            return cached
        }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess, let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else {
            return nil
        }

        cache[key] = value
        return value
    }

    static func delete(key: String) {
        cache.removeValue(forKey: key)

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: servicePrefix,
            kSecAttrAccount as String: key,
        ]

        SecItemDelete(query as CFDictionary)
    }

    // Keys for account session storage
    static func sessionKey(for accountId: UUID) -> String {
        "\(accountId.uuidString)-sessionKey"
    }
}
```

- [ ] **Step 2: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add KeychainService for secure token storage"
```

---

### Task 4: AccountStore

**Files:**
- Create: `ClaudeDashboard/Services/AccountStore.swift`
- Create: `ClaudeDashboardTests/AccountStoreTests.swift`

- [ ] **Step 1: Write failing tests for AccountStore**

Create `ClaudeDashboardTests/AccountStoreTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class AccountStoreTests: XCTestCase {

    private var store: AccountStore!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        defaults = UserDefaults(suiteName: "com.claude-dashboard.tests")!
        defaults.removePersistentDomain(forName: "com.claude-dashboard.tests")
        store = AccountStore(defaults: defaults)
    }

    func testAddAccount() {
        let account = Account(
            id: UUID(),
            name: "Test Account",
            chromeProfilePath: "Profile 1",
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)

        XCTAssertEqual(store.accounts.count, 1)
        XCTAssertEqual(store.accounts.first?.name, "Test Account")
    }

    func testRemoveAccount() {
        let account = Account(
            id: UUID(),
            name: "Test",
            chromeProfilePath: "Profile 1",
            orgId: "org-123",
            plan: .pro,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)
        XCTAssertEqual(store.accounts.count, 1)

        store.removeAccount(id: account.id)
        XCTAssertEqual(store.accounts.count, 0)
    }

    func testUpdateAccount() {
        var account = Account(
            id: UUID(),
            name: "Old Name",
            chromeProfilePath: "Profile 1",
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)
        account.name = "New Name"
        store.updateAccount(account)

        XCTAssertEqual(store.accounts.first?.name, "New Name")
    }

    func testPersistsAcrossInstances() {
        let account = Account(
            id: UUID(),
            name: "Persistent",
            chromeProfilePath: "Profile 1",
            orgId: "org-123",
            plan: .max200,
            lastSynced: nil,
            status: .active
        )

        store.addAccount(account)

        let store2 = AccountStore(defaults: defaults)
        XCTAssertEqual(store2.accounts.count, 1)
        XCTAssertEqual(store2.accounts.first?.name, "Persistent")
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(error:|FAIL|Executed)"
```

Expected: compilation error — `AccountStore` not found.

- [ ] **Step 3: Implement AccountStore**

Create `ClaudeDashboard/Services/AccountStore.swift`:

```swift
import Foundation
import Combine

final class AccountStore: ObservableObject {
    @Published private(set) var accounts: [Account] = []

    private let defaults: UserDefaults
    private let storageKey = "claude-dashboard.accounts"

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.accounts = loadAccounts()
    }

    func addAccount(_ account: Account) {
        accounts.append(account)
        persist()
    }

    func removeAccount(id: UUID) {
        accounts.removeAll { $0.id == id }
        KeychainService.delete(key: KeychainService.sessionKey(for: id))
        persist()
    }

    func updateAccount(_ account: Account) {
        guard let index = accounts.firstIndex(where: { $0.id == account.id }) else { return }
        accounts[index] = account
        persist()
    }

    func saveSessionKey(_ key: String, for accountId: UUID) {
        KeychainService.save(key: KeychainService.sessionKey(for: accountId), value: key)
    }

    func loadSessionKey(for accountId: UUID) -> String? {
        KeychainService.load(key: KeychainService.sessionKey(for: accountId))
    }

    // MARK: - Persistence

    private func persist() {
        guard let data = try? JSONEncoder().encode(accounts) else { return }
        defaults.set(data, forKey: storageKey)
    }

    private func loadAccounts() -> [Account] {
        guard let data = defaults.data(forKey: storageKey),
              let accounts = try? JSONDecoder().decode([Account].self, from: data) else {
            return []
        }
        return accounts
    }
}
```

- [ ] **Step 4: Regenerate project and run tests**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Test Suite|Executed|FAIL)"
```

Expected: `Executed 7 tests, with 0 failures` (3 UsageData + 4 AccountStore)

- [ ] **Step 5: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add AccountStore with UserDefaults persistence"
```

---

### Task 5: ChromeCookieService

**Files:**
- Create: `ClaudeDashboard/Services/ChromeCookieService.swift`
- Create: `ClaudeDashboardTests/ChromeCookieServiceTests.swift`

- [ ] **Step 1: Write failing test for Chrome profile scanning**

Create `ClaudeDashboardTests/ChromeCookieServiceTests.swift`:

```swift
import XCTest
@testable import ClaudeDashboard

final class ChromeCookieServiceTests: XCTestCase {

    func testParsesChromeLocalState() throws {
        let json = """
        {
          "profile": {
            "info_cache": {
              "Default": { "name": "Person 1" },
              "Profile 1": { "name": "Work" },
              "Profile 2": { "name": "Personal" }
            }
          }
        }
        """.data(using: .utf8)!

        let profiles = ChromeCookieService.parseProfiles(from: json)

        XCTAssertEqual(profiles.count, 3)
        XCTAssertEqual(profiles.first(where: { $0.path == "Default" })?.displayName, "Person 1")
        XCTAssertEqual(profiles.first(where: { $0.path == "Profile 1" })?.displayName, "Work")
    }

    func testPBKDF2KeyDerivation() throws {
        // Known test vector: passphrase "test", salt "saltysalt", 1003 iterations
        let key = ChromeCookieService.deriveKey(from: "test")
        XCTAssertEqual(key.count, 16)
        // Just verify it produces consistent output
        let key2 = ChromeCookieService.deriveKey(from: "test")
        XCTAssertEqual(key, key2)
    }

    func testDecryptWithKnownValues() throws {
        // Verify decrypt doesn't crash with valid-shaped input
        // Real decryption requires actual Chrome data, so we test the interface
        let fakeEncrypted = Data([0x76, 0x31, 0x30]) + Data(repeating: 0, count: 32)
        let key = ChromeCookieService.deriveKey(from: "test")
        // Should not crash, may return nil for invalid padding
        let _ = ChromeCookieService.decryptCookieValue(fakeEncrypted, withKey: key)
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(error:|FAIL|Executed)"
```

Expected: compilation error — `ChromeCookieService` not found.

- [ ] **Step 3: Implement ChromeCookieService**

Create `ClaudeDashboard/Services/ChromeCookieService.swift`:

```swift
import Foundation
import CommonCrypto
import SQLite3

struct ChromeProfile {
    let path: String        // e.g. "Profile 1"
    let displayName: String // e.g. "Work"
}

struct ChromeCookieResult {
    let sessionKey: String?
    let orgId: String?
}

enum ChromeCookieService {

    private static let chromeBasePath = NSHomeDirectory()
        + "/Library/Application Support/Google/Chrome"

    // MARK: - Profile Scanning

    static func scanProfiles() -> [ChromeProfile] {
        let localStatePath = chromeBasePath + "/Local State"
        guard let data = FileManager.default.contents(atPath: localStatePath) else {
            return []
        }
        return parseProfiles(from: data)
    }

    static func parseProfiles(from data: Data) -> [ChromeProfile] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let profile = json["profile"] as? [String: Any],
              let infoCache = profile["info_cache"] as? [String: Any] else {
            return []
        }

        return infoCache.compactMap { (key, value) in
            guard let info = value as? [String: Any],
                  let name = info["name"] as? String else {
                return nil
            }
            return ChromeProfile(path: key, displayName: name)
        }
        .sorted { $0.path < $1.path }
    }

    // MARK: - Cookie Extraction

    static func extractCookies(for profilePath: String) -> ChromeCookieResult {
        guard let encryptionKey = getChromeEncryptionKey() else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }

        let dbPath = chromeBasePath + "/\(profilePath)/Cookies"
        var db: OpaquePointer?

        // Open as read-only with WAL mode awareness
        guard sqlite3_open_v2(dbPath, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        defer { sqlite3_close(db) }

        var sessionKey: String?
        var orgId: String?

        let query = """
            SELECT name, encrypted_value FROM cookies
            WHERE (host_key = '.claude.ai' OR host_key = 'claude.ai')
            AND name IN ('sessionKey', 'lastActiveOrg')
        """

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, query, -1, &stmt, nil) == SQLITE_OK else {
            return ChromeCookieResult(sessionKey: nil, orgId: nil)
        }
        defer { sqlite3_finalize(stmt) }

        while sqlite3_step(stmt) == SQLITE_ROW {
            guard let namePtr = sqlite3_column_text(stmt, 0) else { continue }
            let name = String(cString: namePtr)

            let blobSize = sqlite3_column_bytes(stmt, 1)
            guard blobSize > 0,
                  let blobPtr = sqlite3_column_blob(stmt, 1) else { continue }
            let encrypted = Data(bytes: blobPtr, count: Int(blobSize))

            guard let decrypted = decryptCookieValue(encrypted, withKey: encryptionKey) else {
                continue
            }

            switch name {
            case "sessionKey":
                sessionKey = decrypted
            case "lastActiveOrg":
                orgId = decrypted
            default:
                break
            }
        }

        return ChromeCookieResult(sessionKey: sessionKey, orgId: orgId)
    }

    // MARK: - Profiles with Claude Sessions

    static func profilesWithClaudeSessions() -> [(profile: ChromeProfile, cookies: ChromeCookieResult)] {
        let profiles = scanProfiles()
        return profiles.compactMap { profile in
            let cookies = extractCookies(for: profile.path)
            guard cookies.sessionKey != nil else { return nil }
            return (profile: profile, cookies: cookies)
        }
    }

    // MARK: - Crypto

    static func getChromeEncryptionKey() -> Data? {
        let passphrase = getChromeSafeStoragePassword()
        guard let passphrase else { return nil }
        return deriveKey(from: passphrase)
    }

    static func deriveKey(from passphrase: String) -> Data {
        let salt = "saltysalt".data(using: .utf8)!
        var derivedKey = Data(count: 16)

        derivedKey.withUnsafeMutableBytes { derivedKeyBytes in
            salt.withUnsafeBytes { saltBytes in
                CCKeyDerivationPBKDF(
                    CCPBKDFAlgorithm(kCCPBKDF2),
                    passphrase,
                    passphrase.utf8.count,
                    saltBytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    salt.count,
                    CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA1),
                    1003,
                    derivedKeyBytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    16
                )
            }
        }

        return derivedKey
    }

    static func decryptCookieValue(_ encrypted: Data, withKey key: Data) -> String? {
        // Must start with "v10" prefix
        guard encrypted.count > 3,
              encrypted[0] == 0x76, encrypted[1] == 0x31, encrypted[2] == 0x30 else {
            return nil
        }

        let ciphertext = encrypted.dropFirst(3)
        let iv = Data(repeating: 0x20, count: 16) // 16 space bytes

        var decryptedData = Data(count: ciphertext.count + kCCBlockSizeAES128)
        var decryptedLength = 0

        let status = decryptedData.withUnsafeMutableBytes { decryptedBytes in
            ciphertext.withUnsafeBytes { ciphertextBytes in
                key.withUnsafeBytes { keyBytes in
                    iv.withUnsafeBytes { ivBytes in
                        CCCrypt(
                            CCOperation(kCCDecrypt),
                            CCAlgorithm(kCCAlgorithmAES128),
                            CCOptions(kCCOptionPKCS7Padding),
                            keyBytes.baseAddress, key.count,
                            ivBytes.baseAddress,
                            ciphertextBytes.baseAddress, ciphertext.count,
                            decryptedBytes.baseAddress, decryptedData.count,
                            &decryptedLength
                        )
                    }
                }
            }
        }

        guard status == kCCSuccess else { return nil }

        decryptedData.count = decryptedLength

        // Chrome DB v24+ may prepend 32-byte domain hash
        if decryptedLength > 32 {
            let asString = String(data: decryptedData, encoding: .utf8)
            if asString == nil || asString?.contains("\0") == true {
                // Try stripping 32-byte prefix
                let stripped = decryptedData.dropFirst(32)
                if let result = String(data: stripped, encoding: .utf8) {
                    return result
                }
            }
            if let result = asString {
                return result
            }
        }

        return String(data: decryptedData, encoding: .utf8)
    }

    private static func getChromeSafeStoragePassword() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "Chrome Safe Storage",
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess, let data = result as? Data else {
            return nil
        }

        return String(data: data, encoding: .utf8)
    }
}
```

- [ ] **Step 4: Regenerate project and run tests**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Test Suite|Executed|FAIL)"
```

Expected: `Executed 10 tests, with 0 failures`

- [ ] **Step 5: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add ChromeCookieService for profile scanning and cookie decryption"
```

---

### Task 6: UsageAPIService

**Files:**
- Create: `ClaudeDashboard/Services/UsageAPIService.swift`
- Create: `ClaudeDashboardTests/UsageAPIServiceTests.swift`

- [ ] **Step 1: Write failing tests for UsageAPIService**

Create `ClaudeDashboardTests/UsageAPIServiceTests.swift`:

```swift
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
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(error:|FAIL|Executed)"
```

Expected: compilation error — `UsageAPIService` not found.

- [ ] **Step 3: Implement UsageAPIService**

Create `ClaudeDashboard/Services/UsageAPIService.swift`:

```swift
import Foundation

enum UsageAPIError: Error {
    case authExpired
    case httpError(statusCode: Int)
    case invalidResponse
}

struct UsageAPIResult {
    let usage: UsageData
    let newSessionKey: String?
}

final class UsageAPIService {
    private let session: URLSession
    private let baseURL = "https://claude.ai/api/organizations"

    init(session: URLSession = .shared) {
        self.session = session
    }

    func fetchUsage(orgId: String, sessionKey: String) async throws -> UsageAPIResult {
        let url = URL(string: "\(baseURL)/\(orgId)/usage")!
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("*/*", forHTTPHeaderField: "accept")
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue("web_claude_ai", forHTTPHeaderField: "anthropic-client-platform")
        request.setValue("sessionKey=\(sessionKey)", forHTTPHeaderField: "Cookie")

        let (data, response) = try await session.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw UsageAPIError.invalidResponse
        }

        if httpResponse.statusCode == 401 || httpResponse.statusCode == 403 {
            throw UsageAPIError.authExpired
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            throw UsageAPIError.httpError(statusCode: httpResponse.statusCode)
        }

        let usage = try UsageData.decode(from: data)
        let newSessionKey = parseSessionKey(from: httpResponse)

        return UsageAPIResult(usage: usage, newSessionKey: newSessionKey)
    }

    private func parseSessionKey(from response: HTTPURLResponse) -> String? {
        guard let setCookie = response.value(forHTTPHeaderField: "Set-Cookie") else {
            return nil
        }

        // Parse "sessionKey=VALUE; ..." format
        let components = setCookie.components(separatedBy: ";")
        for component in components {
            let trimmed = component.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("sessionKey=") {
                return String(trimmed.dropFirst("sessionKey=".count))
            }
        }

        return nil
    }
}
```

- [ ] **Step 4: Regenerate project and run tests**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Test Suite|Executed|FAIL)"
```

Expected: `Executed 13 tests, with 0 failures`

- [ ] **Step 5: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add UsageAPIService with session refresh support"
```

---

### Task 7: DashboardViewModel

**Files:**
- Create: `ClaudeDashboard/ViewModels/DashboardViewModel.swift`

- [ ] **Step 1: Implement DashboardViewModel**

Create `ClaudeDashboard/ViewModels/DashboardViewModel.swift`:

```swift
import Foundation
import Combine
import SwiftUI

struct AccountUsageState: Identifiable {
    let id: UUID
    let account: Account
    var usage: UsageData?
    var isLoading: Bool = false
    var error: String?
}

@MainActor
final class DashboardViewModel: ObservableObject {
    @Published var accountStates: [AccountUsageState] = []
    @Published var isRefreshing = false

    let accountStore: AccountStore
    private let apiService: UsageAPIService
    private var cancellables = Set<AnyCancellable>()

    init(accountStore: AccountStore = AccountStore(), apiService: UsageAPIService = UsageAPIService()) {
        self.accountStore = accountStore
        self.apiService = apiService

        accountStore.$accounts
            .sink { [weak self] accounts in
                self?.syncStates(with: accounts)
            }
            .store(in: &cancellables)
    }

    // MARK: - Refresh

    func refreshAll() async {
        isRefreshing = true
        defer { isRefreshing = false }

        await withTaskGroup(of: (UUID, UsageData?, String?).self) { group in
            for state in accountStates where state.account.status != .expired {
                let account = state.account
                guard let sessionKey = accountStore.loadSessionKey(for: account.id),
                      let orgId = account.orgId else {
                    continue
                }

                group.addTask { [apiService, accountStore] in
                    do {
                        let result = try await apiService.fetchUsage(orgId: orgId, sessionKey: sessionKey)
                        if let newKey = result.newSessionKey {
                            accountStore.saveSessionKey(newKey, for: account.id)
                        }
                        return (account.id, result.usage, nil)
                    } catch UsageAPIError.authExpired {
                        return (account.id, nil, "expired")
                    } catch {
                        return (account.id, nil, error.localizedDescription)
                    }
                }
            }

            for await (accountId, usage, error) in group {
                if let index = accountStates.firstIndex(where: { $0.id == accountId }) {
                    accountStates[index].usage = usage ?? accountStates[index].usage
                    accountStates[index].error = error

                    if error == "expired" {
                        var account = accountStates[index].account
                        account.status = .expired
                        accountStore.updateAccount(account)
                    } else if error == nil {
                        var account = accountStates[index].account
                        account.status = .active
                        account.lastSynced = Date()
                        accountStore.updateAccount(account)
                    }
                }
            }
        }
    }

    func resyncAccount(_ accountId: UUID) {
        guard let account = accountStore.accounts.first(where: { $0.id == accountId }) else { return }

        let cookies = ChromeCookieService.extractCookies(for: account.chromeProfilePath)
        guard let sessionKey = cookies.sessionKey else { return }

        accountStore.saveSessionKey(sessionKey, for: accountId)

        var updated = account
        updated.status = .active
        if let orgId = cookies.orgId {
            updated.orgId = orgId
        }
        accountStore.updateAccount(updated)
    }

    // MARK: - Menubar Label

    var menuBarLabel: String {
        guard let highest = accountStates
            .compactMap({ $0.usage?.fiveHour })
            .max(by: { $0.utilization < $1.utilization }) else {
            return "--"
        }

        let pct = Int(highest.utilization)
        if let reset = highest.resetsAt {
            let remaining = reset.timeIntervalSinceNow
            if remaining > 0 {
                let h = Int(remaining) / 3600
                let m = (Int(remaining) % 3600) / 60
                return "\(pct)% \u{00B7} \(h)h\(String(format: "%02d", m))m"
            }
        }
        return "\(pct)%"
    }

    // MARK: - Color

    static func usageColor(for utilization: Double) -> Color {
        // HSB interpolation: hue 120° (green) → 0° (red)
        let hue = max(0, min(120, 120 * (1 - utilization / 100))) / 360
        return Color(hue: hue, saturation: 0.7, brightness: 0.85)
    }

    // MARK: - Private

    private func syncStates(with accounts: [Account]) {
        let existingMap = Dictionary(uniqueKeysWithValues: accountStates.map { ($0.id, $0) })
        accountStates = accounts.map { account in
            if let existing = existingMap[account.id] {
                return AccountUsageState(
                    id: account.id,
                    account: account,
                    usage: existing.usage,
                    isLoading: existing.isLoading,
                    error: existing.error
                )
            }
            return AccountUsageState(id: account.id, account: account)
        }
    }
}
```

- [ ] **Step 2: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Run existing tests still pass**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Executed|FAIL)"
```

Expected: `Executed 13 tests, with 0 failures`

- [ ] **Step 4: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add DashboardViewModel with parallel refresh and menubar label"
```

---

### Task 8: UsageBar + AccountCard Views

**Files:**
- Create: `ClaudeDashboard/Views/UsageBar.swift`
- Create: `ClaudeDashboard/Views/AccountCard.swift`

- [ ] **Step 1: Implement UsageBar**

Create `ClaudeDashboard/Views/UsageBar.swift`:

```swift
import SwiftUI

struct UsageBar: View {
    let label: String           // "5h" or "7d"
    let utilization: Double     // 0-100
    let resetsAt: Date?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .leading)

                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.primary.opacity(0.1))

                        RoundedRectangle(cornerRadius: 4)
                            .fill(DashboardViewModel.usageColor(for: utilization))
                            .frame(width: geo.size.width * min(utilization / 100, 1.0))
                    }
                }
                .frame(height: 8)

                Text("\(Int(utilization))%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 36, alignment: .trailing)
            }

            if let resetsAt {
                Text("resets in \(formatTimeRemaining(resetsAt))")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .padding(.leading, 28)
            }
        }
    }

    private func formatTimeRemaining(_ date: Date) -> String {
        let remaining = date.timeIntervalSinceNow
        guard remaining > 0 else { return "now" }

        let days = Int(remaining) / 86400
        let hours = (Int(remaining) % 86400) / 3600
        let minutes = (Int(remaining) % 3600) / 60

        if days > 0 {
            return "\(days)d \(hours)h"
        } else if hours > 0 {
            return "\(hours)h \(String(format: "%02d", minutes))m"
        } else {
            return "\(minutes)m"
        }
    }
}
```

- [ ] **Step 2: Implement AccountCard**

Create `ClaudeDashboard/Views/AccountCard.swift`:

```swift
import SwiftUI

struct AccountCard: View {
    let state: AccountUsageState
    let onResync: () -> Void

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                // Header
                HStack {
                    Text(state.account.name)
                        .font(.headline)

                    Spacer()

                    if state.isLoading {
                        ProgressView()
                            .controlSize(.small)
                    } else if state.account.status == .expired {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    } else if let usage = state.usage {
                        Circle()
                            .fill(DashboardViewModel.usageColor(for: usage.fiveHour.utilization))
                            .frame(width: 10, height: 10)
                    }
                }

                if state.account.status == .expired {
                    expiredContent
                } else if let usage = state.usage {
                    usageContent(usage)
                } else if let error = state.error {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.vertical, 4)
        }
    }

    private func usageContent(_ usage: UsageData) -> some View {
        VStack(spacing: 8) {
            UsageBar(label: "5h", utilization: usage.fiveHour.utilization, resetsAt: usage.fiveHour.resetsAt)
            UsageBar(label: "7d", utilization: usage.sevenDay.utilization, resetsAt: usage.sevenDay.resetsAt)
        }
    }

    private var expiredContent: some View {
        VStack(spacing: 8) {
            Text("Session expired.")
                .font(.caption)
                .foregroundStyle(.secondary)

            Button("Re-sync from Chrome") {
                onResync()
            }
            .controlSize(.small)
        }
    }
}
```

- [ ] **Step 3: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 4: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add UsageBar and AccountCard views with gradient colors"
```

---

### Task 9: MenuBarPopover

**Files:**
- Create: `ClaudeDashboard/Views/MenuBarPopover.swift`
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift`

- [ ] **Step 1: Implement MenuBarPopover**

Create `ClaudeDashboard/Views/MenuBarPopover.swift`:

```swift
import SwiftUI

struct MenuBarPopover: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onOpenWindow: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Claude Dashboard")
                    .font(.headline)

                Spacer()

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .disabled(viewModel.isRefreshing)

                Button(action: onOpenWindow) {
                    Image(systemName: "rectangle.expand.vertical")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            // Account cards
            if viewModel.accountStates.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state) {
                                viewModel.resyncAccount(state.id)
                            }
                        }
                    }
                    .padding(12)
                }
            }
        }
        .frame(width: 320, maxHeight: 500)
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.largeTitle)
                .foregroundStyle(.secondary)

            Text("No accounts configured")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Text("Open Settings to sync from Chrome")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}
```

- [ ] **Step 2: Wire up ClaudeDashboardApp with MenuBarPopover**

Replace `ClaudeDashboard/ClaudeDashboardApp.swift` with:

```swift
import SwiftUI

@main
struct ClaudeDashboardApp: App {
    @StateObject private var viewModel = DashboardViewModel()
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(viewModel: viewModel) {
                openWindow(id: "dashboard")
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chart.bar.fill")
                Text(viewModel.menuBarLabel)
                    .monospacedDigit()
            }
        }
        .menuBarExtraStyle(.window)

        Window("Claude Dashboard", id: "dashboard") {
            Text("Full window — coming in Task 10")
                .frame(width: 600, height: 400)
                .environmentObject(viewModel)
        }
    }
}
```

- [ ] **Step 3: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 4: Run existing tests still pass**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Executed|FAIL)"
```

Expected: `Executed 13 tests, with 0 failures`

- [ ] **Step 5: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add MenuBarPopover with card list and menubar label"
```

---

### Task 10: DashboardWindow (Full Window)

**Files:**
- Create: `ClaudeDashboard/Views/DashboardWindow.swift`
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift`

- [ ] **Step 1: Implement DashboardWindow**

Create `ClaudeDashboard/Views/DashboardWindow.swift`:

```swift
import SwiftUI

struct DashboardWindow: View {
    @ObservedObject var viewModel: DashboardViewModel
    @State private var showingSettings = false

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Text("Claude Dashboard")
                    .font(.title2.bold())

                Spacer()

                Button(action: {
                    Task { await viewModel.refreshAll() }
                }) {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(viewModel.isRefreshing)

                Button(action: { showingSettings = true }) {
                    Label("Settings", systemImage: "gearshape")
                }
            }
            .padding()

            Divider()

            // Cards grid
            if viewModel.accountStates.isEmpty {
                ContentUnavailableView(
                    "No Accounts",
                    systemImage: "person.crop.circle.badge.plus",
                    description: Text("Open Settings to sync accounts from Chrome.")
                )
            } else {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 280, maximum: 400), spacing: 12)],
                        spacing: 12
                    ) {
                        ForEach(viewModel.accountStates) { state in
                            AccountCard(state: state) {
                                viewModel.resyncAccount(state.id)
                            }
                        }
                    }
                    .padding()
                }
            }
        }
        .frame(minWidth: 400, minHeight: 300)
        .sheet(isPresented: $showingSettings) {
            SettingsView(viewModel: viewModel)
        }
    }
}
```

- [ ] **Step 2: Update ClaudeDashboardApp to use DashboardWindow**

Replace the `Window` scene in `ClaudeDashboard/ClaudeDashboardApp.swift`:

Replace:
```swift
        Window("Claude Dashboard", id: "dashboard") {
            Text("Full window — coming in Task 10")
                .frame(width: 600, height: 400)
                .environmentObject(viewModel)
        }
```

With:
```swift
        Window("Claude Dashboard", id: "dashboard") {
            DashboardWindow(viewModel: viewModel)
        }
        .defaultSize(width: 700, height: 500)
```

- [ ] **Step 3: Create placeholder SettingsView so it compiles**

Create `ClaudeDashboard/Views/SettingsView.swift`:

```swift
import SwiftUI

struct SettingsView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack {
            Text("Settings — coming in Task 11")
            Button("Close") { dismiss() }
        }
        .frame(width: 400, height: 300)
        .padding()
    }
}
```

- [ ] **Step 4: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 5: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add DashboardWindow with adaptive grid layout"
```

---

### Task 11: SettingsView + First-time Setup

**Files:**
- Modify: `ClaudeDashboard/Views/SettingsView.swift`
- Create: `ClaudeDashboard/Views/SetupView.swift`
- Modify: `ClaudeDashboard/ClaudeDashboardApp.swift`

- [ ] **Step 1: Implement SetupView (Chrome profile picker)**

Create `ClaudeDashboard/Views/SetupView.swift`:

```swift
import SwiftUI

struct SetupView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @Environment(\.dismiss) private var dismiss

    @State private var detectedProfiles: [(profile: ChromeProfile, cookies: ChromeCookieResult)] = []
    @State private var selectedProfiles: Set<String> = []
    @State private var accountNames: [String: String] = [:]
    @State private var accountPlans: [String: AccountPlan] = [:]
    @State private var isScanning = false
    @State private var scanError: String?

    var body: some View {
        VStack(spacing: 16) {
            Text("Setup — Sync from Chrome")
                .font(.title2.bold())

            if isScanning {
                ProgressView("Scanning Chrome profiles...")
            } else if detectedProfiles.isEmpty {
                noProfilesView
            } else {
                profileList
            }

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)

                Spacer()

                if !detectedProfiles.isEmpty {
                    Button("Add Selected") {
                        addSelectedAccounts()
                        dismiss()
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(selectedProfiles.isEmpty)
                }
            }
        }
        .padding(24)
        .frame(width: 500, minHeight: 300)
        .onAppear { scan() }
    }

    private var noProfilesView: some View {
        VStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle")
                .font(.largeTitle)
                .foregroundStyle(.orange)

            Text(scanError ?? "No Chrome profiles found with active Claude sessions.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)

            Button("Retry Scan") { scan() }
                .padding(.top, 8)
        }
    }

    private var profileList: some View {
        List {
            ForEach(detectedProfiles, id: \.profile.path) { item in
                HStack {
                    Toggle(isOn: Binding(
                        get: { selectedProfiles.contains(item.profile.path) },
                        set: { isOn in
                            if isOn {
                                selectedProfiles.insert(item.profile.path)
                            } else {
                                selectedProfiles.remove(item.profile.path)
                            }
                        }
                    )) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(item.profile.displayName)
                                .font(.body)
                            Text("Chrome: \(item.profile.path)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Spacer()

                    if selectedProfiles.contains(item.profile.path) {
                        TextField("Account name", text: Binding(
                            get: { accountNames[item.profile.path] ?? item.profile.displayName },
                            set: { accountNames[item.profile.path] = $0 }
                        ))
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 140)

                        Picker("", selection: Binding(
                            get: { accountPlans[item.profile.path] ?? .max200 },
                            set: { accountPlans[item.profile.path] = $0 }
                        )) {
                            ForEach(AccountPlan.allCases, id: \.self) { plan in
                                Text(plan.rawValue)
                            }
                        }
                        .frame(width: 70)
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private func scan() {
        isScanning = true
        scanError = nil

        DispatchQueue.global(qos: .userInitiated).async {
            let results = ChromeCookieService.profilesWithClaudeSessions()

            DispatchQueue.main.async {
                self.detectedProfiles = results
                self.isScanning = false

                if results.isEmpty {
                    self.scanError = "No Chrome profiles found with active Claude sessions. Make sure you're logged into claude.ai in your Chrome profiles."
                }
            }
        }
    }

    private func addSelectedAccounts() {
        for item in detectedProfiles where selectedProfiles.contains(item.profile.path) {
            // Skip if already mapped
            if viewModel.accountStore.accounts.contains(where: { $0.chromeProfilePath == item.profile.path }) {
                continue
            }

            let name = accountNames[item.profile.path] ?? item.profile.displayName
            let plan = accountPlans[item.profile.path] ?? .max200

            let account = Account(
                id: UUID(),
                name: name,
                chromeProfilePath: item.profile.path,
                orgId: item.cookies.orgId,
                plan: plan,
                lastSynced: Date(),
                status: .active
            )

            viewModel.accountStore.addAccount(account)

            if let sessionKey = item.cookies.sessionKey {
                viewModel.accountStore.saveSessionKey(sessionKey, for: account.id)
            }
        }
    }
}
```

- [ ] **Step 2: Implement full SettingsView**

Replace `ClaudeDashboard/Views/SettingsView.swift` with:

```swift
import SwiftUI

struct SettingsView: View {
    @ObservedObject var viewModel: DashboardViewModel
    @Environment(\.dismiss) private var dismiss
    @State private var showingSetup = false
    @State private var editingAccount: Account?

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Settings")
                    .font(.title2.bold())
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding()

            Divider()

            // Account list
            List {
                Section("Accounts") {
                    ForEach(viewModel.accountStore.accounts) { account in
                        accountRow(account)
                    }
                }
            }

            Divider()

            // Footer
            HStack {
                Button(action: { showingSetup = true }) {
                    Label("Add from Chrome", systemImage: "plus.circle")
                }

                Spacer()

                Button("Re-sync All from Chrome") {
                    for account in viewModel.accountStore.accounts {
                        viewModel.resyncAccount(account.id)
                    }
                }
            }
            .padding()
        }
        .frame(width: 500, height: 400)
        .sheet(isPresented: $showingSetup) {
            SetupView(viewModel: viewModel)
        }
        .sheet(item: $editingAccount) { account in
            EditAccountView(account: account) { updated in
                viewModel.accountStore.updateAccount(updated)
            }
        }
    }

    private func accountRow(_ account: Account) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(account.name)
                    .font(.body)
                HStack(spacing: 4) {
                    Text(account.plan.rawValue)
                        .font(.caption)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.secondary.opacity(0.15))
                        .clipShape(Capsule())

                    Text(account.chromeProfilePath)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            if account.status == .expired {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
            }

            Button(action: { editingAccount = account }) {
                Image(systemName: "pencil")
            }
            .buttonStyle(.borderless)

            Button(action: {
                viewModel.accountStore.removeAccount(id: account.id)
            }) {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .foregroundStyle(.red)
        }
        .padding(.vertical, 2)
    }
}

struct EditAccountView: View {
    @State var account: Account
    let onSave: (Account) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 16) {
            Text("Edit Account")
                .font(.headline)

            TextField("Name", text: $account.name)
                .textFieldStyle(.roundedBorder)

            Picker("Plan", selection: $account.plan) {
                ForEach(AccountPlan.allCases, id: \.self) { plan in
                    Text(plan.rawValue)
                }
            }

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Save") {
                    onSave(account)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 300)
    }
}
```

- [ ] **Step 3: Update ClaudeDashboardApp for first-time setup**

Replace `ClaudeDashboard/ClaudeDashboardApp.swift` with:

```swift
import SwiftUI

@main
struct ClaudeDashboardApp: App {
    @StateObject private var viewModel = DashboardViewModel()
    @Environment(\.openWindow) private var openWindow
    @State private var showingSetup = false

    var body: some Scene {
        MenuBarExtra {
            MenuBarPopover(viewModel: viewModel) {
                openWindow(id: "dashboard")
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "chart.bar.fill")
                Text(viewModel.menuBarLabel)
                    .monospacedDigit()
            }
        }
        .menuBarExtraStyle(.window)

        Window("Claude Dashboard", id: "dashboard") {
            DashboardWindow(viewModel: viewModel)
                .onAppear {
                    if viewModel.accountStore.accounts.isEmpty {
                        showingSetup = true
                    }
                }
                .sheet(isPresented: $showingSetup) {
                    SetupView(viewModel: viewModel)
                }
        }
        .defaultSize(width: 700, height: 500)
    }
}
```

- [ ] **Step 4: Regenerate project and build**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodegen generate && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -3
```

Expected: `BUILD SUCCEEDED`

- [ ] **Step 5: Run all tests still pass**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Executed|FAIL)"
```

Expected: `Executed 13 tests, with 0 failures`

- [ ] **Step 6: Commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: add SettingsView and SetupView with Chrome profile picker"
```

---

### Task 12: Integration Test — Manual Smoke Test

- [ ] **Step 1: Build and launch the app**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard -configuration Debug build 2>&1 | tail -3
```

```bash
open /Users/haicao/code/others/claude-dashboard/build/Debug/ClaudeDashboard.app
```

- [ ] **Step 2: Verify menubar icon appears**

Expected: A `chart.bar.fill` icon appears in the macOS menubar with text `--`.

- [ ] **Step 3: Click menubar icon, verify popover shows empty state**

Expected: Popover shows "No accounts configured" with prompt to open Settings.

- [ ] **Step 4: Open full window, verify setup sheet appears**

Expected: Full window opens and automatically shows the Setup sheet since no accounts exist.

- [ ] **Step 5: Verify Chrome profile detection works**

Expected: Setup view detects Chrome profiles with active Claude sessions. User can select profiles, name them, and add them.

- [ ] **Step 6: After adding accounts, click Refresh**

Expected: Usage data loads for each account. Cards show 5h and 7d progress bars with percentages and reset countdowns. Colors interpolate from green to red based on 5h usage.

- [ ] **Step 7: Verify full window grid layout**

Expected: Cards display in 2-column grid when window is wide enough, single column when narrow.

- [ ] **Step 8: Run full test suite one final time**

```bash
cd /Users/haicao/code/others/claude-dashboard && xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | grep -E "(Test Suite|Executed|FAIL)"
```

Expected: All tests pass.

- [ ] **Step 9: Final commit**

```bash
cd /Users/haicao/code/others/claude-dashboard && git add -A && git commit -m "feat: Claude Dashboard v1.0 — multi-account usage monitor"
```

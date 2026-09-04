import Foundation

enum AccountPlan: String, Codable, CaseIterable {
    case pro = "Pro"
    case max5x = "Max 5x"
    case max20x = "Max 20x"
    case max200 = "Max"  // fallback when tier unknown
}

enum AccountStatus: String, Codable {
    case active
    case expired
    case error
}

/// Where this record's session key comes from. `manual` means the user pasted
/// it: there is no browser profile behind the record, so `chromeProfilePath` is
/// empty and `browser` is meaningless.
///
/// Consumers key on this field alone. An empty `chromeProfilePath` is **not** a
/// secondary signal for "manual" — that sentinel is what this field replaces.
enum AccountSource: String, Codable {
    case browser
    case manual
}

struct Account: Identifiable, Codable, Equatable {
    let id: UUID
    var name: String
    var email: String?
    var chromeProfilePath: String
    var chromeProfileName: String?
    var orgId: String?
    /// The Claude account's own uuid, from `GET /api/account`. This is the
    /// identity key — `orgId` is a query parameter and identifies nothing.
    /// Optional because records written before this field existed have no value;
    /// it is backfilled on the next successful refresh.
    var accountUuid: String?
    var sessionKey: String?
    var browser: Browser = .chrome
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus
    var isPinned: Bool = false
    /// No default on purpose: the memberwise initializer must force every
    /// construction site to say which source it is creating. A default of
    /// `.browser` would let a manual add silently produce a browser record.
    var source: AccountSource

    var isConfigured: Bool {
        orgId != nil
    }
}

// Custom decode để tương thích JSON cũ (thiếu key "browser" → .chrome).
// LƯU Ý: giữ CodingKeys và init(from:) đồng bộ với mọi stored property của Account;
// thêm property mới mà quên cập nhật ở đây sẽ âm thầm mất dữ liệu khi round-trip.
extension Account {
    private enum CodingKeys: String, CodingKey {
        case id, name, email, chromeProfilePath, chromeProfileName
        case orgId, accountUuid, sessionKey, browser, plan, lastSynced, status, isPinned, source
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UUID.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        email = try c.decodeIfPresent(String.self, forKey: .email)
        chromeProfilePath = try c.decode(String.self, forKey: .chromeProfilePath)
        chromeProfileName = try c.decodeIfPresent(String.self, forKey: .chromeProfileName)
        orgId = try c.decodeIfPresent(String.self, forKey: .orgId)
        accountUuid = try c.decodeIfPresent(String.self, forKey: .accountUuid)
        sessionKey = try c.decodeIfPresent(String.self, forKey: .sessionKey)
        browser = try c.decodeIfPresent(Browser.self, forKey: .browser) ?? .chrome
        plan = try c.decode(AccountPlan.self, forKey: .plan)
        lastSynced = try c.decodeIfPresent(Date.self, forKey: .lastSynced)
        status = try c.decode(AccountStatus.self, forKey: .status)
        isPinned = try c.decodeIfPresent(Bool.self, forKey: .isPinned) ?? false
        source = try c.decodeIfPresent(AccountSource.self, forKey: .source) ?? .browser
    }
}

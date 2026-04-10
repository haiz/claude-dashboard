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

struct Account: Identifiable, Codable, Equatable {
    let id: UUID
    var name: String
    var email: String?
    var chromeProfilePath: String
    var chromeProfileName: String?
    var orgId: String?
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus

    var isConfigured: Bool {
        orgId != nil
    }
}

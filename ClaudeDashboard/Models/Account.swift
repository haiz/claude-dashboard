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

import Foundation
import SwiftUI

enum AccountPlan: String, Codable, CaseIterable {
    case pro = "Pro"
    case max5x = "Max 5x"
    case max20x = "Max 20x"
    case max200 = "Max"  // fallback when tier unknown

    var badgeColor: Color {
        switch self {
        case .pro: return .blue
        case .max5x: return .purple
        case .max20x: return .orange
        case .max200: return .purple
        }
    }
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
    var sessionKey: String?
    var plan: AccountPlan
    var lastSynced: Date?
    var status: AccountStatus
    var isPinned: Bool = false

    var isConfigured: Bool {
        orgId != nil
    }
}

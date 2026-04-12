import SwiftUI

extension AccountPlan {
    var badgeColor: Color {
        switch self {
        case .pro: return .blue
        case .max5x: return .purple
        case .max20x: return .orange
        case .max200: return .purple
        }
    }
}

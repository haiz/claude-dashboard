import SwiftUI

/// One row of the main window's sidebar. `.account` carries only the id, so the
/// row keeps one identity while that account's usage changes.
///
///     viewModel.selection = .settingsGeneral
///     SidebarItem.overview.style?.title   // "Overview"
enum SidebarItem: Hashable {
    case dashboard
    case overview
    case account(UUID)
    case commandLog
    case help
    case settingsAccounts
    case settingsGeneral

    struct Style {
        let title: String
        let systemImage: String
        let tileColor: Color
    }

    /// Title, SF Symbol and tile color of a static row. nil for `.account`, whose
    /// row is drawn from the account itself (avatar, name, peak percent).
    var style: Style? {
        switch self {
        case .dashboard: return Style(title: "Dashboard", systemImage: "square.grid.2x2", tileColor: .blue)
        case .overview: return Style(title: "Overview", systemImage: "chart.xyaxis.line", tileColor: .orange)
        case .account: return nil
        case .commandLog: return Style(title: "Command Log", systemImage: "list.bullet.rectangle", tileColor: .indigo)
        case .help: return Style(title: "Help", systemImage: "questionmark", tileColor: .green)
        case .settingsAccounts: return Style(title: "Accounts", systemImage: "person.2", tileColor: .teal)
        case .settingsGeneral: return Style(title: "General", systemImage: "gearshape", tileColor: .gray)
        }
    }

    static let usageItems: [SidebarItem] = [.dashboard, .overview]
    static let toolItems: [SidebarItem] = [.commandLog, .help]
    static let settingsItems: [SidebarItem] = [.settingsAccounts, .settingsGeneral]
}

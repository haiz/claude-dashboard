import XCTest
import SwiftUI
import AppKit
@testable import ClaudeDashboard

/// A compact card must fit the menu bar popover's width less its card list
/// padding. A card that cannot shrink that far overflows the scroll view,
/// which then shows it flush left with a gap on the right.
@MainActor
final class AccountCardLayoutTests: XCTestCase {

    private static let popoverCardWidth = MenuBarPopover.width - 2 * MenuBarPopover.cardListPadding

    func testCompactCardWithAllGaugesFitsThePopover() {
        let resetsAt = Date().addingTimeInterval(3600)
        let usage = UsageData(
            fiveHour: UsageLimit(utilization: 38, resetsAt: resetsAt),
            sevenDay: UsageLimit(utilization: 82, resetsAt: resetsAt.addingTimeInterval(86400 * 3)),
            fable: UsageLimit(utilization: 4, resetsAt: resetsAt)
        )
        let account = Account(
            id: UUID(), name: "a@b.co", email: "a@b.co",
            chromeProfilePath: "Profile 1", chromeProfileName: nil,
            orgId: nil, accountUuid: nil, plan: .max5x, lastSynced: nil,
            status: .active, isPinned: false, source: .browser
        )
        let card = AccountCard(
            state: AccountUsageState(id: account.id, account: account, usage: usage),
            onResync: {}, onTogglePin: {}, isCompact: true
        )

        let host = NSHostingController(rootView: card)
        let size = host.sizeThatFits(in: CGSize(width: Self.popoverCardWidth, height: 10_000))

        XCTAssertLessThanOrEqual(size.width, Self.popoverCardWidth,
                                 "compact card needs \(size.width)pt, popover gives \(Self.popoverCardWidth)pt")
    }
}

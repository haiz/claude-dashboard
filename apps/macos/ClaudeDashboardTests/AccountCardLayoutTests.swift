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
        // The reset labels are fixed-size monospaced text, so their width follows
        // the wall clock. Pin both to their longest form ("12:49 AM", "Wed 12:50am")
        // rather than deriving them from now, which passes or fails by time of day.
        let cal = Calendar.current
        let now = Date()
        let fiveHourReset = cal.nextDate(after: now, matching: DateComponents(hour: 0, minute: 49),
                                         matchingPolicy: .nextTime)!
        let sevenDayReset = cal.nextDate(after: now.addingTimeInterval(86400), matching: DateComponents(hour: 0, minute: 50),
                                         matchingPolicy: .nextTime)!
        let usage = UsageData(
            fiveHour: UsageLimit(utilization: 38, resetsAt: fiveHourReset),
            sevenDay: UsageLimit(utilization: 82, resetsAt: sevenDayReset),
            fable: UsageLimit(utilization: 4, resetsAt: sevenDayReset)
        )
        let account = Account(
            id: UUID(), name: "a@b.co", email: "a@b.co",
            chromeProfilePath: "Profile 1", chromeProfileName: nil,
            orgId: nil, accountUuid: nil, plan: .max5x, lastSynced: nil,
            status: .active, isPinned: false, source: .browser
        )
        // The animal phase of the shared clock (7 s into the cycle): the widest the 5h center
        // can be. The cycle starts when the first gauge mounts, so time moves after that.
        var t = 0.0
        var tick: (@MainActor () -> Void)?
        let clock = AnimalDisplayClock(now: { t }, scheduler: { _, next in tick = next; return {} })
        clock.shouldRun = { true }
        clock.gaugeAppeared()
        t = 7
        tick?()
        XCTAssertEqual(clock.phase.content, .animal)
        let card = AccountCard(
            state: AccountUsageState(id: account.id, account: account, usage: usage),
            onResync: {}, onTogglePin: {},
            burnAnimal: BurnAnimalSource(clock: clock, assetID: { "rabbit" }), isCompact: true
        )

        let host = NSHostingController(rootView: card)
        let size = host.sizeThatFits(in: CGSize(width: Self.popoverCardWidth, height: 10_000))

        XCTAssertLessThanOrEqual(size.width, Self.popoverCardWidth,
                                 "compact card needs \(size.width)pt, popover gives \(Self.popoverCardWidth)pt")
    }
}

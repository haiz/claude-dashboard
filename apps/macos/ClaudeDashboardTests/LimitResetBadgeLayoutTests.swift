import SwiftUI
import XCTest
@testable import ClaudeDashboard

/// The badge sits beside the label without changing the label row's height,
/// so a 7d / F column with a badge keeps its rings level with the 5h column.
@MainActor
final class LimitResetBadgeLayoutTests: XCTestCase {
    func testBadgeKeepsGaugeHeight() throws {
        let summary = ResetSummary(count: 12, expiresAt: .distantFuture, urgent: false)
        for compact in [true, false] {
            let plain = try height(UsageBar(label: "7d", utilization: 40, resetsAt: nil, isCompact: compact))
            let badged = try height(UsageBar(label: "7d", utilization: 40, resetsAt: nil, isCompact: compact,
                                             resetBadge: (summary, "Full resets")))
            XCTAssertEqual(badged, plain, "compact=\(compact)")
        }
    }

    private func height(_ view: UsageBar) throws -> Int {
        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        return try XCTUnwrap(renderer.cgImage).height
    }
}

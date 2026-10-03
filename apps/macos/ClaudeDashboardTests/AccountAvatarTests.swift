import XCTest
@testable import ClaudeDashboard

final class AccountAvatarTests: XCTestCase {
    func testPaletteIndexIsFixedForAnId() {
        let id = UUID(uuidString: "00000000-0000-0000-0000-000000000005")!
        XCTAssertEqual(AccountAvatar.paletteIndex(for: id), 5)
    }

    func testPaletteIndexSpreadsAcrossAccounts() {
        let indices = Set((0..<50).map { _ in AccountAvatar.paletteIndex(for: UUID()) })
        XCTAssertGreaterThan(indices.count, 6)
        XCTAssertTrue(indices.allSatisfy { AccountAvatar.palette.indices.contains($0) })
    }
}

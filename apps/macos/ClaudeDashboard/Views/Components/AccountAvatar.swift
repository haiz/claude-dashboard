import SwiftUI

/// A circle holding the account's initial, tinted with a color picked from the
/// account's id, so each account keeps one color across launches and neighbours
/// in the sidebar differ even when they share a plan.
///
///     AccountAvatar(account: state.account, size: 20)   // sidebar row
///     AccountAvatar(account: state.account, size: 56)   // account pane header
struct AccountAvatar: View {
    let account: Account
    let size: CGFloat

    static let palette: [Color] = [.blue, .orange, .green, .pink, .teal, .indigo, .red, .mint, .purple, .brown, .cyan, .yellow]

    /// Index into `palette` for `id`. Summed from the uuid bytes, not `hashValue`,
    /// which Swift reseeds on every launch.
    static func paletteIndex(for id: UUID) -> Int {
        withUnsafeBytes(of: id.uuid) { $0.reduce(0) { $0 + Int($1) } } % palette.count
    }

    private var initial: String {
        account.name.first.map { String($0).uppercased() } ?? "?"
    }

    var body: some View {
        Circle()
            .fill(Self.palette[Self.paletteIndex(for: account.id)].gradient)
            .frame(width: size, height: size)
            .overlay(
                Text(initial)
                    .font(.system(size: size * 0.45, weight: .semibold, design: .rounded))
                    .foregroundStyle(.white)
            )
    }
}

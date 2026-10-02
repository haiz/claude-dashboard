import SwiftUI

/// A circle holding the account's initial, tinted with its plan badge color.
///
///     AccountAvatar(account: state.account, size: 20)   // sidebar row
///     AccountAvatar(account: state.account, size: 56)   // account pane header
struct AccountAvatar: View {
    let account: Account
    let size: CGFloat

    private var initial: String {
        account.name.first.map { String($0).uppercased() } ?? "?"
    }

    var body: some View {
        Circle()
            .fill(account.plan.badgeColor.gradient)
            .frame(width: size, height: size)
            .overlay(
                Text(initial)
                    .font(.system(size: size * 0.45, weight: .semibold, design: .rounded))
                    .foregroundStyle(.white)
            )
    }
}

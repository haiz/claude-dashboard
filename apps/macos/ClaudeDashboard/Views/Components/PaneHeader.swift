import SwiftUI

/// Title row at the top of every main-window pane, with that pane's buttons on the
/// trailing side. The window has no native toolbar (see the spec), so actions
/// live here, the way System Settings keeps them inside the content.
///
///     PaneHeader(title: "Command Log") {
///         Button("Clear All") { confirmingClear = true }
///     }
///     PaneHeader(title: "About")
struct PaneHeader<Trailing: View>: View {
    let title: String
    let trailing: Trailing

    init(title: String, @ViewBuilder trailing: () -> Trailing) {
        self.title = title
        self.trailing = trailing()
    }

    var body: some View {
        HStack(spacing: 8) {
            Text(title)
                .font(.title2.bold())
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer()
            trailing
        }
        .padding(.horizontal, 20)
        .padding(.top, 14)
        .padding(.bottom, 8)
    }
}

extension PaneHeader where Trailing == EmptyView {
    init(title: String) {
        self.init(title: title) { EmptyView() }
    }
}

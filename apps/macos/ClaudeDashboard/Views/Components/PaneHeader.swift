import SwiftUI

/// Title row at the top of every main-window pane, with that pane's buttons on the
/// trailing side. The title sits on the titlebar row; the buttons go in the window
/// toolbar `NavigationSplitView` puts there (its sidebar toggle), because that toolbar
/// covers the row and swallows mouse clicks on any content button drawn under it.
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
        Text(title)
            .font(.title2.bold())
            .lineLimit(1)
            .truncationMode(.middle)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.top, 14)
            .padding(.bottom, 8)
            .toolbar {
                // `.primaryAction` is the leading edge on macOS; a toolbar Spacer is a
                // flexible space, which pushes the buttons to the trailing edge.
                ToolbarItemGroup {
                    Spacer()
                    trailing.labelStyle(.titleAndIcon)
                }
            }
    }
}

extension PaneHeader where Trailing == EmptyView {
    init(title: String) {
        self.init(title: title) { EmptyView() }
    }
}

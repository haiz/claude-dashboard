import SwiftUI

/// Segmented 5h / 7d / F chooser for the chart toolbars. Replaces a `.segmented`
/// `Picker`, which has no hover state and takes no per-segment tooltip.
///
///     UsageWindowPicker(selection: $selectedWindow)
struct UsageWindowPicker: View {
    @Binding var selection: UsageWindow

    private static let windows: [UsageWindow] = [.fiveHour, .sevenDay, .fable]

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Self.windows, id: \.self) { window in
                Button {
                    selection = window
                } label: {
                    Text(window.label)
                        .font(.callout)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(HoverableButtonStyle(isSelected: selection == window,
                                                  horizontalPadding: 0, verticalPadding: 3, cornerRadius: 5))
                .help("Show the \(Self.windowName(window)) usage window")
            }
        }
        .padding(2)
        .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
    }

    private static func windowName(_ window: UsageWindow) -> String {
        switch window {
        case .fiveHour: return "5-hour"
        case .sevenDay: return "7-day"
        case .fable: return "Fable"
        }
    }
}

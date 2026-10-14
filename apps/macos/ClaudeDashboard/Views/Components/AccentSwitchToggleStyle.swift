import SwiftUI

/// A switch drawn in SwiftUI so its on state keeps the accent color. AppKit's `.switch` greys
/// its track whenever its window is not key, and the menu bar panel loses key status as soon
/// as any other app takes focus, so an on toggle there read as off.
///
///     Toggle("Auto switch", isOn: $isOn)
///         .toggleStyle(AccentSwitchToggleStyle())
struct AccentSwitchToggleStyle: ToggleStyle {
    private static let trackSize = CGSize(width: 30, height: 16)
    private static let thumbInset: CGFloat = 1.5

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            configuration.label
            Capsule()
                .fill(configuration.isOn ? Color.accentColor : Color.primary.opacity(0.2))
                .frame(width: Self.trackSize.width, height: Self.trackSize.height)
                .overlay(alignment: configuration.isOn ? .trailing : .leading) {
                    Circle()
                        .fill(Color.white)
                        .shadow(color: .black.opacity(0.25), radius: 0.5, y: 0.5)
                        .padding(Self.thumbInset)
                }
                .animation(.easeOut(duration: 0.15), value: configuration.isOn)
                .contentShape(Capsule())
                .onTapGesture { configuration.isOn.toggle() }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityValue(configuration.isOn ? "On" : "Off")
        .accessibilityAction { configuration.isOn.toggle() }
    }
}

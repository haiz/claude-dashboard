import SwiftUI

/// Plain button with a hover and pressed background. `isSelected` tints it with
/// the accent color, for segmented-style choices such as chart range presets.
///
///     Button("24h") { apply(.day) }
///         .buttonStyle(HoverableButtonStyle(isSelected: preset == .day))
struct HoverableButtonStyle: ButtonStyle {
    var prominent: Bool = false
    var isSelected: Bool = false
    var horizontalPadding: CGFloat = 10
    var verticalPadding: CGFloat = 5
    var cornerRadius: CGFloat = 6

    func makeBody(configuration: Configuration) -> some View {
        HoverableLabel(
            configuration: configuration,
            prominent: prominent,
            isSelected: isSelected,
            horizontalPadding: horizontalPadding,
            verticalPadding: verticalPadding,
            cornerRadius: cornerRadius
        )
    }

    private struct HoverableLabel: View {
        let configuration: ButtonStyleConfiguration
        let prominent: Bool
        let isSelected: Bool
        let horizontalPadding: CGFloat
        let verticalPadding: CGFloat
        let cornerRadius: CGFloat

        @State private var isHovered = false
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .padding(.horizontal, horizontalPadding)
                .padding(.vertical, verticalPadding)
                .background(
                    RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                        .fill(backgroundColor)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                        .stroke(Color.primary.opacity(prominent ? 0.08 : 0), lineWidth: 0.5)
                )
                .opacity(isEnabled ? 1.0 : 0.45)
                .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
                .contentShape(Rectangle())
                .onHover { hovering in
                    guard isEnabled else { return }
                    isHovered = hovering
                }
                .animation(.easeOut(duration: 0.12), value: isHovered)
                .animation(.easeOut(duration: 0.08), value: configuration.isPressed)
        }

        private var backgroundColor: Color {
            if !isEnabled { return Color.primary.opacity(prominent ? 0.05 : 0) }
            if configuration.isPressed { return Color.primary.opacity(0.18) }
            if isSelected { return Color.accentColor.opacity(isHovered ? 0.28 : 0.2) }
            if isHovered { return Color.primary.opacity(prominent ? 0.16 : 0.10) }
            return Color.primary.opacity(prominent ? 0.07 : 0)
        }
    }
}

struct HoverableRowStyle: ButtonStyle {
    var selected: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        HoverableRow(configuration: configuration, selected: selected)
    }

    private struct HoverableRow: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        @State private var isHovered = false
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .background(
                    Group {
                        if selected {
                            Color.accentColor.opacity(0.12)
                        } else if isHovered && isEnabled {
                            Color.primary.opacity(configuration.isPressed ? 0.12 : 0.07)
                        } else {
                            Color.clear
                        }
                    }
                )
                .contentShape(Rectangle())
                .onHover { isHovered = $0 }
                .animation(.easeOut(duration: 0.10), value: isHovered)
        }
    }
}

/// The system's own button look (`.automatic`, so bordered, default-action and
/// destructive variants stay native) plus a hover highlight. Set once on a root
/// view; buttons with an explicit style keep theirs.
///
///     MainWindow(...).buttonStyle(HoverBorderedButtonStyle())
struct HoverBorderedButtonStyle: PrimitiveButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        HoverBorderedButton(configuration: configuration)
    }

    private struct HoverBorderedButton: View {
        let configuration: PrimitiveButtonStyleConfiguration
        @State private var isHovered = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.colorScheme) private var colorScheme

        var body: some View {
            Button(configuration)
                .buttonStyle(.automatic)
                // Lighten in dark mode, darken in light mode: a white button can't get lighter.
                .brightness(isHovered && isEnabled ? (colorScheme == .dark ? 0.08 : -0.05) : 0)
                .onHover { isHovered = $0 }
                .animation(.easeOut(duration: 0.12), value: isHovered)
        }
    }
}

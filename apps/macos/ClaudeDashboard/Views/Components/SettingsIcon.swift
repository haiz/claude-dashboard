import SwiftUI

/// System Settings-style icon: a white SF Symbol on a colored rounded tile.
///
///     SettingsIcon(systemImage: "gearshape", color: .gray)             // sidebar row
///     SettingsIcon(systemImage: "sparkles", color: .blue, size: 24)   // help section
struct SettingsIcon: View {
    let systemImage: String
    let color: Color
    var size: CGFloat = 20

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.25, style: .continuous)
            .fill(color.gradient)
            .frame(width: size, height: size)
            .overlay(
                Image(systemName: systemImage)
                    .font(.system(size: size * 0.55, weight: .semibold))
                    .foregroundStyle(.white)
            )
    }
}

import SwiftUI

/// An info icon for a setting: hover shows `tip.summary`; click opens the detail, an
/// example and a "More in Help" link.
///
///     LabeledContent { stepper } label: {
///         HStack { Text("Switch at 5h usage"); InfoTip(SettingsTips.fiveHourThreshold) { openHelp(.autoSwitch) } }
///     }
struct InfoTip: View {
    let tip: SettingTip
    let onMoreHelp: (() -> Void)?
    @State private var isShowing = false

    init(_ tip: SettingTip, onMoreHelp: (() -> Void)? = nil) {
        self.tip = tip
        self.onMoreHelp = onMoreHelp
    }

    var body: some View {
        Button { isShowing.toggle() } label: {
            Image(systemName: "info.circle").foregroundStyle(.secondary)
        }
        .buttonStyle(.borderless)
        .help(tip.summary)
        .accessibilityLabel("About this setting")
        .popover(isPresented: $isShowing, arrowEdge: .trailing) {
            VStack(alignment: .leading, spacing: 10) {
                Text(tip.detail)
                Text("Example").font(.caption.bold()).foregroundStyle(.secondary)
                Text(tip.example).foregroundStyle(.secondary)
                if let onMoreHelp {
                    Button("More in Help") {
                        isShowing = false
                        onMoreHelp()
                    }
                    .buttonStyle(.link)
                }
            }
            .fixedSize(horizontal: false, vertical: true)
            .padding(14)
            .frame(width: 340, alignment: .leading)
        }
    }
}

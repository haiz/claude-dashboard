import SwiftUI

/// Building blocks for the Help pane. Every text argument is a `LocalizedStringKey`,
/// so `**bold**` and `` `code` `` render as Markdown.
///
///     HelpCard(title: "Ping", subtitle: "Start a window now", icon: "bolt", color: .indigo) {
///         HelpSteps(["Open the account page.", "Click **Ping**."])
///         HelpCallout(.tip, "Turn on **Auto Ping** to do this for you.")
///     }
struct HelpCard<Content: View>: View {
    let title: String
    let subtitle: String
    let icon: String
    let color: Color
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                SettingsIcon(systemImage: icon, color: color, size: 30)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(.headline)
                    Text(subtitle)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            Divider()
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.06))
        )
    }
}

/// A small sub-heading inside a `HelpCard`.
///
///     HelpSubheading("In the menu bar")
struct HelpSubheading: View {
    let text: LocalizedStringKey
    init(_ text: LocalizedStringKey) { self.text = text }

    var body: some View {
        Text(text)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.top, 2)
    }
}

/// A paragraph of body text that wraps.
///
///     HelpText("Usage refreshes every few minutes.")
struct HelpText: View {
    let text: LocalizedStringKey
    init(_ text: LocalizedStringKey) { self.text = text }

    var body: some View {
        Text(text)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// Numbered steps in a colored badge.
///
///     HelpSteps(["Sign in.", "Click **Add Account…**."])
struct HelpSteps: View {
    let steps: [LocalizedStringKey]
    var color: Color = .accentColor
    init(_ steps: [LocalizedStringKey], color: Color = .accentColor) {
        self.steps = steps
        self.color = color
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(steps.indices, id: \.self) { index in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text("\(index + 1)")
                        .font(.caption.bold().monospacedDigit())
                        .foregroundStyle(.white)
                        .frame(width: 18, height: 18)
                        .background(Circle().fill(color))
                        .alignmentGuide(.firstTextBaseline) { $0[.bottom] - 5 }
                    Text(steps[index])
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}

/// One row of a legend: a fixed-width visual on the left, the explanation on the right.
///
///     HelpRow(symbol: "pin.fill", "**Pinned**: kept at the top of the list.")
///     HelpRow("Plan badge") { Text("Max").font(.caption.bold()) }
struct HelpRow<Visual: View>: View {
    let text: LocalizedStringKey
    let visual: Visual

    init(_ text: LocalizedStringKey, @ViewBuilder visual: () -> Visual) {
        self.text = text
        self.visual = visual()
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            visual
                .frame(width: 44, alignment: .center)
            Text(text)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }
}

extension HelpRow where Visual == HelpSymbol {
    init(symbol: String, color: Color = .secondary, _ text: LocalizedStringKey) {
        self.init(text) { HelpSymbol(name: symbol, color: color) }
    }
}

/// An SF Symbol sized for a `HelpRow`.
struct HelpSymbol: View {
    let name: String
    let color: Color

    var body: some View {
        Image(systemName: name)
            .font(.body)
            .foregroundStyle(color)
    }
}

/// A short label in a rounded box, mimicking the app's small text badges ("5h", "Max").
///
///     HelpBadge("5h")
struct HelpBadge: View {
    let text: String
    var color: Color = .secondary
    init(_ text: String, color: Color = .secondary) {
        self.text = text
        self.color = color
    }

    var body: some View {
        Text(text)
            .font(.caption.bold().monospaced())
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.15))
            .clipShape(Capsule())
    }
}

/// A tinted note box: a tip, a warning, or a plain note.
///
///     HelpCallout(.warning, "Don't sign the same account in twice.")
struct HelpCallout: View {
    enum Kind {
        case tip, note, warning

        var icon: String {
            switch self {
            case .tip: return "lightbulb.fill"
            case .note: return "info.circle.fill"
            case .warning: return "exclamationmark.triangle.fill"
            }
        }

        var color: Color {
            switch self {
            case .tip: return .yellow
            case .note: return .blue
            case .warning: return .orange
            }
        }
    }

    let kind: Kind
    let text: LocalizedStringKey
    init(_ kind: Kind, _ text: LocalizedStringKey) {
        self.kind = kind
        self.text = text
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: kind.icon)
                .foregroundStyle(kind.color)
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(10)
        .background(kind.color.opacity(0.1), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

/// A question with its answer, for Troubleshooting.
///
///     HelpFAQ("Usage is not updating", "Click **Refresh**.")
struct HelpFAQ: View {
    let question: LocalizedStringKey
    let answer: LocalizedStringKey
    init(_ question: LocalizedStringKey, _ answer: LocalizedStringKey) {
        self.question = question
        self.answer = answer
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label {
                Text(question).font(.body.weight(.semibold))
            } icon: {
                Image(systemName: "questionmark.circle.fill").foregroundStyle(.secondary)
            }
            Text(answer)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.leading, 26)
        }
    }
}

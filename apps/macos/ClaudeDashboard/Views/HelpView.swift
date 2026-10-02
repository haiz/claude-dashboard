import SwiftUI

struct HelpView: View {
    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Help")
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    gettingStartedSection
                    readingUsageSection
                    dashboardSection
                    managingAccountsSection
                    autoRefreshSection
                    troubleshootingSection
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 20)
            }
        }
    }

    // MARK: - Sections

    private var gettingStartedSection: some View {
        helpSection(title: "Getting Started", icon: "sparkles", color: .blue) {
            bodyText("Claude Dashboard shows your Claude.ai token usage across multiple accounts, pulled directly from your Chrome sessions — no passwords required.")

            VStack(alignment: .leading, spacing: 6) {
                step(number: 1, text: "Sign in to claude.ai in Google Chrome (one profile per account).")
                step(number: 2, text: "Click the Claude Dashboard icon in the menu bar, then click **Add Account**.")
                step(number: 3, text: "Select the Chrome profiles you want to track and click **Add Selected**.")
            }

            tip("When macOS asks for Keychain access, choose **Always Allow** — the app uses this to decrypt Chrome's cookie store.")
        }
    }

    private var readingUsageSection: some View {
        helpSection(title: "Reading Your Usage", icon: "chart.bar.fill", color: .purple) {
            bodyText("Each account card shows up to three usage bars:")

            VStack(alignment: .leading, spacing: 6) {
                bullet(icon: nil, text: "**5h** — tokens used in the rolling 5-hour window.")
                bullet(icon: nil, text: "**7d** — tokens used in the rolling 7-day window.")
                bullet(icon: nil, text: "**F** — 7-day usage for the Fable model (shown only when your plan scopes it).")
            }

            bodyText("Bar color signals how much is left: green (plenty), yellow (halfway), red (nearly out). The countdown next to each bar shows when that window resets.")

            bodyText("The animal emoji reflects your burn rate — a sloth means you're using tokens slowly; a cheetah means you're burning through them fast.")

            bodyText("A green dot on an account name means that account is currently active in Claude Code.")
        }
    }

    private var dashboardSection: some View {
        helpSection(title: "Working With the Dashboard", icon: "square.grid.2x2", color: .indigo) {
            bodyText("The menu bar popover header has five icon buttons, left to right:")

            VStack(alignment: .leading, spacing: 6) {
                bullet(icon: "arrow.clockwise",            text: "**Refresh** — fetch the latest usage now.")
                bullet(icon: "rectangle.expand.vertical",  text: "**Expand** — open the full dashboard window.")
                bullet(icon: "chart.xyaxis.line",          text: "**Overview** — see all accounts on one chart.")
                bullet(icon: "questionmark.circle",        text: "**Help** — this screen.")
                bullet(icon: "gearshape",                  text: "**Settings** — accounts, refresh, and updates.")
            }

            bodyText("Click any usage bar on a card to open that account's **Detail Chart**. Use the **5h / 7d / S** picker to change the time range, **Show All** to reset the zoom, and **Measure** to compare usage between two points.")

            bodyText("In **Overview**, toggle individual accounts on and off using the legend at the bottom.")

            bodyText("**Right-click** a card to pin it to the top of the list regardless of burn rate.")

            bodyText("The terminal icon on a card opens **Run Command** — enter any shell command and it runs using that account's context, then the dashboard refreshes.")
        }
    }

    private var managingAccountsSection: some View {
        helpSection(title: "Managing Accounts", icon: "person.2.circle", color: .green) {
            VStack(alignment: .leading, spacing: 6) {
                bullet(icon: "plus.circle",  text: "**Add an account** — **Settings › Accounts** → **Add Account**, then pick your browser (Chrome, Arc, Brave, Edge).")
                bullet(icon: "arrow.clockwise", text: "**Re-sync** — **Settings › Accounts** → **Re-sync All**, or use the **Re-sync** button on any expired card.")
                bullet(icon: "trash",        text: "**Remove an account** — **Settings › Accounts** → trash icon next to the account, or **Remove Account…** on the account's page.")
            }

            tip("If a card shows an orange triangle, the session has expired. Open the matching browser profile, log into claude.ai, then re-sync.")
        }
    }

    private var autoRefreshSection: some View {
        helpSection(title: "Auto-Refresh & Updates", icon: "arrow.triangle.2.circlepath", color: .orange) {
            bodyText("Enable **Auto Refresh** under **Settings › General** to keep usage current automatically. Set the interval anywhere from 1 to 60 minutes.")

            bodyText("**Auto-update daily** checks GitHub once a day and installs new releases in the background. Use **Check for Updates** to force a check immediately.")
        }
    }

    private var troubleshootingSection: some View {
        helpSection(title: "Troubleshooting", icon: "wrench.and.screwdriver", color: .gray) {
            VStack(alignment: .leading, spacing: 10) {
                problemAnswer(
                    problem: "\"No profiles found\"",
                    answer: "The browser must be running with that profile open. Only profiles the browser currently considers active are scanned. Click **Retry Scan** after opening the browser."
                )
                problemAnswer(
                    problem: "Account stuck on \"Session expired\"",
                    answer: "Open the matching browser profile, log into claude.ai, then click **Re-sync** on the card."
                )
                problemAnswer(
                    problem: "macOS blocks the app on first launch",
                    answer: "Go to System Settings → Privacy & Security, scroll down, and click **Open Anyway**. Alternatively, right-click the app in Finder and choose **Open**."
                )
                problemAnswer(
                    problem: "Usage not updating",
                    answer: "Click the Refresh button in the popover. Make sure you have an active claude.ai session in Chrome for each tracked account."
                )
            }
        }
    }

    // MARK: - Helpers

    private func helpSection<Content: View>(title: String, icon: String, color: Color, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                SettingsIcon(systemImage: icon, color: color, size: 24)
                Text(title)
                    .font(.headline)
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.06))
        )
    }

    private func bodyText(_ text: LocalizedStringKey) -> some View {
        Text(text)
            .font(.body)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func step(number: Int, text: LocalizedStringKey) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("\(number).")
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 16, alignment: .trailing)
            Text(text)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func bullet(icon: String?, text: LocalizedStringKey) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if let icon {
                Image(systemName: icon)
                    .font(.caption)
                    .frame(width: 16, alignment: .center)
                    .foregroundStyle(.secondary)
            } else {
                Text("•")
                    .foregroundStyle(.secondary)
                    .frame(width: 16, alignment: .center)
            }
            Text(text)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func tip(_ text: LocalizedStringKey) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: "lightbulb")
                .font(.caption)
                .foregroundStyle(.yellow)
            Text(text)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(8)
        .background(.yellow.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private func problemAnswer(problem: String, answer: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(problem)
                .font(.body.weight(.medium))
            Text(answer)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

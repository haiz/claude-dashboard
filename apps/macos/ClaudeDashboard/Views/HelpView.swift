import SwiftUI

/// Tools > Help: one card per topic, with a topic grid at the top that scrolls to it.
/// Blocks come from `HelpBlocks.swift`. Keep the wording in step with the UI labels
/// it names (button titles, Settings section names).
///
///     HelpView(focus: $vm.helpFocus)
struct HelpView: View {
    @Binding var focus: HelpTopic?

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Help")
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        topicGrid(proxy)
                        ForEach(HelpTopic.allCases) { topic in
                            HelpCard(title: topic.title, subtitle: topic.subtitle,
                                     icon: topic.icon, color: topic.color) {
                                content(for: topic)
                            }
                            .id(topic)
                        }
                    }
                    .padding(.horizontal, 20)
                    .padding(.bottom, 20)
                }
                .onAppear { scrollToFocus(proxy) }
                .onChange(of: focus) { _ in scrollToFocus(proxy) }
            }
        }
    }

    private func scrollToFocus(_ proxy: ScrollViewProxy) {
        guard let topic = focus else { return }
        DispatchQueue.main.async {
            withAnimation { proxy.scrollTo(topic, anchor: .top) }
            focus = nil
        }
    }

    // MARK: - Topic grid

    private func topicGrid(_ proxy: ScrollViewProxy) -> some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 8)], spacing: 8) {
            ForEach(HelpTopic.allCases) { topic in
                TopicChip(topic: topic) {
                    withAnimation { proxy.scrollTo(topic, anchor: .top) }
                }
            }
        }
    }

    // MARK: - Topics

    @ViewBuilder
    private func content(for topic: HelpTopic) -> some View {
        switch topic {
        case .gettingStarted: gettingStarted
        case .readingCard: readingCard
        case .findingYourWay: findingYourWay
        case .charts: charts
        case .switchClaudeCode: switchClaudeCode
        case .switchClaudeDesktop: switchClaudeDesktop
        case .autoSwitch: autoSwitch
        case .criticalAccounts: criticalAccounts
        case .ping: ping
        case .accounts: accounts
        case .refreshUpdates: refreshUpdates
        case .troubleshooting: troubleshooting
        }
    }

    private var gettingStarted: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("Claude Dashboard reads your claude.ai sessions straight from your browser. No passwords, nothing to type.")
            HelpSteps([
                "Sign in to claude.ai in **Chrome, Chrome Dev, Arc, Brave or Edge**. Use one browser profile per account.",
                "Open **Settings › Accounts** and click **Add Account…**",
                "Pick your browser, check the accounts it finds, then click **Add Selected**.",
            ])
            HelpCallout(.tip, "When macOS asks for Keychain access, choose **Always Allow**. The app needs it to read the browser's cookies.")
            HelpCallout(.note, "No browser session? In the same window, click **Paste a session key instead** and paste the account's claude.ai `sessionKey` cookie.")
        }
    }

    private var readingCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center, spacing: 20) {
                UsageBar(label: "5h", utilization: 62, resetsAt: Date().addingTimeInterval(8_100),
                         totalSeconds: 18_000, animal: "🐇", isCompact: false)
                    .allowsHitTesting(false)
                VStack(alignment: .leading, spacing: 8) {
                    HelpText("**Big ring**: how much of the window you have used, with the percentage inside.")
                    HelpText("**Small ring**: time until the window resets. One segment per hour (5h) or per day (7d). It turns green as the reset gets close.")
                    HelpText("**Animal**: how fast you are burning (see below).")
                }
            }

            HelpSubheading("The windows")
            HelpRow("**5-hour window**: usage in the current 5-hour session.") { HelpBadge("5h") }
            HelpRow("**7-day window**: your weekly usage across all models.") { HelpBadge("7d") }
            HelpRow("**Fable**: weekly Fable usage. Shown only when your plan has a Fable limit; it resets together with 7d.") { HelpBadge("F") }
            HelpRow("**Color** goes from green (plenty left) to red (almost out).") { usageGradient }

            HelpSubheading("Burn rate: time until you hit 100% at the current pace")
            burnRateScale

            HelpSubheading("Badges on a card")
            HelpRow("**Active in Claude Code**: the account Claude Code is using right now.") {
                Circle().fill(.green).frame(width: 8, height: 8)
            }
            HelpRow("**Plan**: Pro, Max 5x, Max 20x, or Max when the tier is unknown.") {
                HelpBadge("Pro", color: AccountPlan.pro.badgeColor)
            }
            HelpRow(symbol: "pin.fill", "**Pinned**: always at the top of the list, and the account shown in the menu bar.")
            HelpRow(symbol: "bolt.fill", color: .orange, "**Home after reset**: Claude Code comes back here each time this account's 5h limit resets.")
            HelpRow("**Flame**: orange when the account is critical, red when high critical: its weekly quota will go unused. Hover for the numbers.") {
                HelpSymbol(name: "flame.fill", color: .red)
            }
            HelpRow("**X** (Dashboard header): what a full 5h window costs of the 7d quota. Click for its history.") {
                HelpSymbol(name: "chart.xyaxis.line", color: .orange)
            }
            HelpRow(symbol: "exclamationmark.triangle.fill", color: .orange, "**Session expired**: sign in again in the browser, then **Re-sync**.")

            HelpCallout(.note, "The menu bar shows the 5h usage and the time left until reset for your pinned account, or the top account when none is pinned.")
        }
    }

    private var findingYourWay: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpSubheading("Menu bar popover")
            HelpRow(symbol: "arrow.clockwise", "**Refresh**: fetch the latest usage for every account.")
            HelpRow(symbol: "rectangle.expand.vertical", "**Open window**: the full dashboard.")
            HelpRow(symbol: "questionmark.circle", "**Help**: this page.")
            HelpRow(symbol: "gearshape", "**Settings**: accounts, refresh, Auto Ping, Claude Desktop, Auto Switch, updates.")
            HelpRow(symbol: "power", "**Quit** (bottom left). The **Auto switch** toggle sits bottom right with its latest event.")

            HelpSubheading("Main window sidebar")
            HelpRow(symbol: "square.grid.2x2", color: .blue, "**Usage**: Dashboard (all cards) and Overview (all accounts on one chart).")
            HelpRow(symbol: "person.crop.circle", color: .teal, "**Accounts**: one page per account, with its current usage next to the name.")
            HelpRow(symbol: "wrench.and.screwdriver", color: .indigo, "**Tools**: Ping Log, Switch Log and Help.")
            HelpRow(symbol: "gearshape", color: .gray, "**Settings**: Accounts and General.")

            HelpCallout(.tip, "Click any ring to open that account's page on that window's chart. **Right-click** a card to pin it to the top.")
        }
    }

    private var charts: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("Each account page has a **History** chart.")
            HelpRow(symbol: "rectangle.split.3x1", "**Window**: pick 5h, 7d or Fable.")
            HelpRow("**Range**: the last 5h, 24h, 3d, 7d or 30d.") { HelpBadge("24h") }
            HelpRow(symbol: "arrow.up.and.down.and.arrow.left.and.right", "**Pan / zoom**: drag the chart to move through time; the mode button changes what a drag does.")
            HelpRow(symbol: "list.bullet", "**Reset cycles**: click one to show only that cycle; **Show All** brings back the rest.")
            HelpRow(symbol: "ruler", "**Measure**: click two points to see how much usage changed between them.")
            HelpCallout(.tip, "**Overview** puts every account on one chart. Click an account in the legend to show or hide it.")
        }
    }

    private var switchClaudeCode: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("Click \(Image(systemName: "arrow.left.arrow.right")) on a card, or **Switch** on the account page, then confirm. Claude Code now uses that account.")
            HelpSubheading("The first time for an account")
            HelpSteps([
                "The app opens **that account's own browser profile** on Claude's sign-in page.",
                "Sign in and click **Authorize**.",
                "The switch finishes by itself. From then on it is instant, no browser, for about 30 days.",
            ], color: .pink)
            HelpRow(symbol: "bolt", color: .orange, "**Switch after reset**: tick \(Image(systemName: "bolt")) on a card (or the toggle on its page) to move Claude Code there each time its 5h limit resets. The tick stays until you untick it, and the active account can be ticked too. Only one account can be ticked.")
            HelpCallout(.warning, "Don't also use the same account through another `CLAUDE_CONFIG_DIR`. Two copies of one login sign each other out.")
        }
    }

    private var switchClaudeDesktop: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("Turn on **Switch Claude Desktop with Switch** in **Settings › General › Claude Desktop** (off by default). **Switch** then moves Claude Desktop to the same account as Claude Code.")
            HelpRow("**Desktop open**: the confirm box gets **Switch Claude Desktop too**, already ticked. Desktop must be closed to switch, so the app asks first: **Close and Switch** quits it (unsaved work in Desktop may be lost). Untick the box to move only Claude Code.") {
                HelpSymbol(name: "macwindow", color: .mint)
            }
            HelpRow("**Desktop closed**: it is switched straight away, with no question. Open it and it starts on the new account.") {
                HelpSymbol(name: "moon.zzz", color: .secondary)
            }
            HelpSubheading("The first time for an account")
            HelpText("Claude Desktop gets **its own login**, separate from Claude Code's, so the two never sign each other out. The first switch to an account opens its browser profile once more: sign in and click **Authorize**. After that it is instant for about 30 days, and each refresh keeps Desktop's login fresh.")
            HelpRow(symbol: "arrow.left.arrow.right", color: .mint, "**Switch Desktop to this account**: on the page of the account Claude Code uses, shown when Desktop is on another account. Use it to move Desktop later, for example after unticking the box mid-conversation.")
            HelpRow(symbol: "arrow.triangle.swap", color: .teal, "**Let auto switch move Desktop when it is closed** (off by default): an auto switch also moves Desktop, only while Desktop is closed and only to an account Desktop has signed in to before. A running Desktop is never quit.")
            HelpCallout(.note, "If Desktop is signed in to an account that is not on the dashboard, the switch leaves Desktop alone so that login is not lost. Add that account first.")

            HelpSubheading("Recommended: Claude Code and Claude Desktop")
            HelpSteps([
                "Turn on **Switch Claude Desktop with Switch** and **Let auto switch move Desktop when it is closed**. Keep **Auto Switch** on.",
                "Once, while you have a few minutes, **Switch** to each account with **Switch Claude Desktop too** ticked and authorize it. Auto switch can then move Desktop to any of them without a browser.",
                "Quit Desktop when you step away. An auto switch then moves it too, so it opens on the same account as Claude Code.",
                "Mid-conversation in Desktop? Untick **Switch Claude Desktop too** and move it later with **Switch Desktop to this account**.",
            ], color: .mint)

            HelpSubheading("Recommended: Claude Desktop only")
            HelpSteps([
                "Turn **Auto Switch** off: it follows Claude Code's account, not Desktop's (see **Auto Switch**). Leave **Let auto switch move Desktop when it is closed** off too.",
                "Turn on **Switch Claude Desktop with Switch**. **Switch** also moves Claude Code; that does no harm if you don't use it.",
                "**Pin** the account Desktop uses (right-click its card): the menu bar then shows its 5h usage, so you see a limit coming.",
                "Switch to each account once ahead of time. The first time asks you to authorize twice, once for Claude Code and once for Desktop; later switches need no browser.",
                "Before you press **Switch**, finish or save what you are doing in Desktop, then close it yourself.",
            ], color: .mint)

            HelpCallout(.tip, "Add accounts from the browser, not with a pasted key: the first switch needs the account's own browser profile. About once a month an account's login runs out and its next switch opens the browser again; that is expected.")
        }
    }

    private var criticalAccounts: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("An account is **critical** when its weekly (7d) quota will expire unused even if you keep using it steadily. The app marks it with a flame and, with **Auto Switch** on, moves Claude Code to it as soon as its 5h window is free.")
            HelpSubheading("X: what a 5h window costs")
            HelpText("**X** (Dashboard header) is the share of the 7d quota one full 5h window uses, measured every hour from the last 3 days of usage across all accounts. Click it for its history. Until enough history exists it shows **16% (default)**.")
            HelpSubheading("The two levels")
            HelpRow("**Critical** (orange flame): using **Critical pace** (default 50%) of every 5h window from now to the weekly reset, skipping **Quiet hours** (default 00:00 to 07:00, Asia/Ho_Chi_Minh), the account still ends at or under 100% of 7d.") {
                HelpSymbol(name: "flame", color: .orange)
            }
            HelpRow("**High critical** (red flame): even **High critical pace** (default 80%) around the clock, quiet hours included, ends at or under 100%.") {
                HelpSymbol(name: "flame.fill", color: .red)
            }
            HelpCallout(.note, "Formula: 7d now + (usable hours left / 5) x pace x X. Usable hours leave out quiet hours for Critical only.")
            HelpSubheading("Who wins")
            HelpSteps(["**High critical**: takes Claude Code from any account that is not high critical, the home account (bolt) included.",
                       "**Home account**: comes back after its 5h reset, unless Claude Code is on a high critical account.",
                       "**Critical**: takes Claude Code from an ordinary account that is not the home account.",
                       "**Thresholds**: the usual 5h/7d switch."])
            HelpCallout(.note, "After a critical switch or a manual **Switch**, Claude Code stays on that account until its 5h window resets, unless a 5h/7d limit or the home account's reset moves it.")
            HelpSubheading("Examples (X = 16%)")
            HelpText("**1.** 7d at 55%, 25 usable hours left (5 windows). Critical pace 50% costs 8% per window: 55 + 40 = **95**, so critical, 5% would go unused.")
            HelpText("**2.** 7d at 60%, reset in 48 hours from 10:00, quiet 00:00 to 07:00: 34 usable hours (6.8 windows) x 8% = 54.4, so 60 + 54.4 = **114.4**: not critical. At 7d 40% it is 94.4: critical.")
            HelpText("**3.** 7d at 60%, reset in 10 hours. High critical pace 80% costs 12.8% per window: 60 + 25.6 = **85.6**, so high critical. If Claude Code is on the home account, it moves here when this account's 5h window is free.")
            HelpCallout(.tip, "Change the paces, quiet hours and time zone in **Settings › General › Critical Accounts**. Each setting's ⓘ shows another example.")
        }
    }

    private var autoSwitch: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("Auto Switch is **on by default**. When the account Claude Code uses reaches a limit, Claude Code moves to another account without a click.")
            HelpRow("**When**: 5h usage reaches **95%** or 7d usage reaches **99%**. Change both in **Settings › General › Auto Switch**.") {
                HelpSymbol(name: "gauge.with.dots.needle.67percent", color: .orange)
            }
            HelpRow("**Where to**: the account whose weekly quota is closest to going unused, so nothing is wasted. Near-equal choices are picked at random.") {
                HelpSymbol(name: "target", color: .teal)
            }
            HelpRow("**First, critical accounts**: a high critical account takes Claude Code from any account that is not high critical, the home account (bolt) included; the home account comes back after its 5h reset; and a critical account takes Claude Code from an ordinary account that is not the home account. See **Critical Accounts**.") {
                HelpSymbol(name: "flame", color: .red)
            }
            HelpRow("**Near a 5h reset**: on the home account or the most critical account, 5h at 95% with 5 minutes or less to its reset waits for the reset when your pace of the last few minutes would not reach 100%, so Claude Code does not leave and come straight back.") {
                HelpSymbol(name: "timer", color: .blue)
            }
            HelpRow("**Which accounts**: only ones already signed in to Claude Code (through Switch or `/login`), so the switch is instant. It never opens a browser.") {
                HelpSymbol(name: "checkmark.seal", color: .green)
            }
            HelpRow("**On failure**: that account is skipped for 10 minutes.") {
                HelpSymbol(name: "clock.arrow.circlepath", color: .secondary)
            }
            HelpRow("**Claude Desktop**: moved only with **Let auto switch move Desktop when it is closed** on, and only while Desktop is closed. A running Desktop stays where it is.") {
                HelpSymbol(name: "macwindow", color: .mint)
            }
            HelpCallout(.warning, "**Only use Claude Desktop?** Turn Auto Switch off. It watches the account **Claude Code** uses, and Desktop is open while you work, so it never moves Desktop away from a limit. It can still move Claude Code, and a closed Desktop, to an account picked from Claude Code's usage, not yours.")
            HelpCallout(.tip, "**Switch Log** (Tools) lists every switch and every failed attempt, and why nothing happened when no account was available. Claude Desktop rows carry a **Desktop** tag.")
        }
    }

    private var ping: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpText("A window only starts counting at your first message. Pinging starts it now, so it resets sooner.")
            HelpRow("**Ping** (account page): sends one short message from that account to start its 5h and 7d windows.") {
                HelpSymbol(name: "hand.tap", color: .indigo)
            }
            HelpRow("**Auto Ping** (Settings › General, off by default): after each refresh, pings every active account whose 5h or 7d window is not running.") {
                HelpSymbol(name: "repeat", color: .indigo)
            }
            HelpRow("**Ping Log** (Tools): the latest 500 pings and their results.") {
                HelpSymbol(name: "list.bullet.rectangle", color: .indigo)
            }
        }
    }

    private var accounts: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpRow(symbol: "plus.circle", color: .green, "**Add**: **Settings › Accounts** → **Add Account…**")
            HelpRow(symbol: "arrow.triangle.2.circlepath", color: .blue, "**Re-sync**: read a fresh session key from the browser. One account from its page, or every account with **Re-sync All**.")
            HelpRow(symbol: "pin", color: .secondary, "**Pin to Top**: right-click a card, or the toggle on the account page.")
            HelpRow(symbol: "trash", color: .red, "**Remove**: the trash icon in **Settings › Accounts**, or **Remove Account…** on the account page.")
            HelpCallout(.note, "Account names come from the account's email (or the browser profile name) and update on each sync.")
        }
    }

    private var refreshUpdates: some View {
        VStack(alignment: .leading, spacing: 12) {
            HelpRow(symbol: "arrow.clockwise", "**Auto Refresh** (Settings › General): keeps usage current, every 1 to 60 minutes.")
            HelpRow(symbol: "arrow.down.circle", "**Auto-update daily**: checks GitHub once a day and installs new releases. **Check for Updates** checks right now.")
        }
    }

    private var troubleshooting: some View {
        VStack(alignment: .leading, spacing: 14) {
            HelpFAQ("\"No profiles found\" when adding",
                    "Make sure you are signed in to claude.ai in that browser, pick the right browser at the top, then click **Retry Scan**.")
            HelpFAQ("A card says \"Session expired\"",
                    "Open the browser profile named on the card, sign in to claude.ai, then click **Re-sync**. A pasted key can't be re-synced: add it again with **Paste a session key instead**.")
            HelpFAQ("Switch asks me to run /login",
                    "Claude Code lost that login. Run `/login` in Claude Code and sign in with that account.")
            HelpFAQ("Switch is refused",
                    "Claude Code is signed in to an account that is not on the dashboard. Add that account first so its login is kept safe.")
            HelpFAQ("Claude Desktop was not switched",
                    "Desktop must be closed to switch: agree to **Close and Switch**, or close it and use **Switch Desktop to this account**. If Desktop is signed in to an account that is not on the dashboard, add that account first. The switch message says which case it was.")
            HelpFAQ("Auto Switch did nothing",
                    "Check **Switch Log**. Usually no account was ready: switch to each account once by hand, or every other account is also at its limit. Within 5 minutes of a 5h reset, the home or most critical account waits for the reset on purpose.")
            HelpFAQ("Usage is not updating",
                    "Click **Refresh** and check **Auto Refresh** in Settings › General. Expired accounts are skipped until you re-sync them.")
            HelpFAQ("macOS blocks the app on first launch",
                    "Open **System Settings › Privacy & Security**, scroll down and click **Open Anyway**. Or right-click the app in Finder and choose **Open**.")
        }
    }

    // MARK: - Visuals

    private var usageGradient: some View {
        Capsule()
            .fill(LinearGradient(colors: [0, 50, 100].map { DashboardViewModel.usageColor(for: $0) },
                                 startPoint: .leading, endPoint: .trailing))
            .frame(width: 40, height: 8)
    }

    /// Mirrors `BurnRateResult.fromProjectedTime`.
    private var burnRateScale: some View {
        HStack(spacing: 6) {
            ForEach(Self.burnRateLevels, id: \.level) { item in
                VStack(spacing: 2) {
                    Text(BurnRateResult.animals[item.level] ?? "")
                        .font(.title2)
                    Text(item.range)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            }
        }
    }

    private static let burnRateLevels: [(level: Int, range: String)] = [
        (1, "over 5h"), (2, "3-5h"), (3, "1.5-3h"), (4, "30m-1.5h"), (5, "under 30m"),
    ]
}

/// A tile in the Help pane's topic grid; clicking it scrolls to that topic's card.
private struct TopicChip: View {
    let topic: HelpTopic
    let action: () -> Void
    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                SettingsIcon(systemImage: topic.icon, color: topic.color, size: 20)
                Text(topic.title)
                    .lineLimit(1)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity)
            .background(Color.primary.opacity(isHovered ? 0.10 : 0.04),
                        in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
        .help("Jump to \(topic.title)")
    }
}

/// The Help pane's topics, in display order.
enum HelpTopic: String, CaseIterable, Identifiable {
    case gettingStarted, readingCard, findingYourWay, charts, switchClaudeCode, switchClaudeDesktop
    case autoSwitch, criticalAccounts, ping, accounts, refreshUpdates, troubleshooting

    var id: String { rawValue }

    var title: String {
        switch self {
        case .gettingStarted: return "Getting Started"
        case .readingCard: return "Reading a Card"
        case .findingYourWay: return "Finding Your Way"
        case .charts: return "Charts"
        case .switchClaudeCode: return "Switch Claude Code"
        case .switchClaudeDesktop: return "Switch Claude Desktop"
        case .autoSwitch: return "Auto Switch"
        case .criticalAccounts: return "Critical Accounts"
        case .ping: return "Ping"
        case .accounts: return "Managing Accounts"
        case .refreshUpdates: return "Refresh & Updates"
        case .troubleshooting: return "Troubleshooting"
        }
    }

    var subtitle: String {
        switch self {
        case .gettingStarted: return "Add your first account in under a minute."
        case .readingCard: return "What the rings, colors, animals and badges mean."
        case .findingYourWay: return "The menu bar popover and the main window."
        case .charts: return "Look back at how each account was used."
        case .switchClaudeCode: return "Move Claude Code to another account in one click."
        case .switchClaudeDesktop: return "Move Claude Desktop along with Claude Code."
        case .autoSwitch: return "Let the app switch before you hit a limit."
        case .criticalAccounts: return "Spend weekly quota before it expires unused."
        case .ping: return "Start a usage window now so it resets sooner."
        case .accounts: return "Add, re-sync, pin and remove accounts."
        case .refreshUpdates: return "Keep usage and the app up to date."
        case .troubleshooting: return "Common problems and how to fix them."
        }
    }

    var icon: String {
        switch self {
        case .gettingStarted: return "sparkles"
        case .readingCard: return "gauge.with.dots.needle.50percent"
        case .findingYourWay: return "menubar.rectangle"
        case .charts: return "chart.xyaxis.line"
        case .switchClaudeCode: return "arrow.left.arrow.right"
        case .switchClaudeDesktop: return "macwindow"
        case .autoSwitch: return "arrow.triangle.swap"
        case .criticalAccounts: return "flame"
        case .ping: return "dot.radiowaves.left.and.right"
        case .accounts: return "person.2"
        case .refreshUpdates: return "arrow.clockwise"
        case .troubleshooting: return "wrench.and.screwdriver"
        }
    }

    var color: Color {
        switch self {
        case .gettingStarted: return .blue
        case .readingCard: return .purple
        case .findingYourWay: return .gray
        case .charts: return .orange
        case .switchClaudeCode: return .pink
        case .switchClaudeDesktop: return .mint
        case .autoSwitch: return .teal
        case .criticalAccounts: return .red
        case .ping: return .indigo
        case .accounts: return .green
        case .refreshUpdates: return .cyan
        case .troubleshooting: return .brown
        }
    }
}

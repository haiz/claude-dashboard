import Foundation

/// One setting's explanation: `summary` is the hover tooltip; the click popover shows
/// `detail`, then `example`, then a link to `topic` in Help.
struct SettingTip: Equatable {
    let summary: String
    let detail: String
    let example: String
    let topic: HelpTopic
}

/// The text of every `InfoTip` in Settings > General. Keep it in step with the Help topics.
enum SettingsTips {
    static let autoRefresh = SettingTip(
        summary: "Fetch usage for every account on a timer.",
        detail: "When on, the app fetches usage for all accounts every interval. When off, usage only updates when you press Refresh (auto switch still polls the active account near a limit).",
        example: "With a 5 min interval, a card that shows 40% at 10:00 shows the new figure by 10:05.",
        topic: .refreshUpdates)
    static let refreshInterval = SettingTip(
        summary: "Minutes between automatic refreshes (1 to 60).",
        detail: "Shorter intervals keep cards fresher and let auto switch react sooner, at the cost of more requests to claude.ai.",
        example: "At 1 min, auto switch sees a 5h window reach 95% within a minute; at 15 min it can be 15 minutes late.",
        topic: .refreshUpdates)
    static let switchDesktop = SettingTip(
        summary: "Also move Claude Desktop when you press Switch.",
        detail: "When on, Switch also signs Claude Desktop in to the same account as Claude Code. A running Desktop is closed only after you agree; a closed one is switched straight away. The first switch to an account opens its browser profile so you can authorize Desktop.",
        example: "Switching to B moves Claude Code and Claude Desktop to B; with this off only Claude Code moves.",
        topic: .switchClaudeDesktop)
    static let autoSwitchDesktop = SettingTip(
        summary: "Let auto switch move Claude Desktop when it is not running.",
        detail: "When on, an auto switch also moves Claude Desktop, but only while Desktop is closed and only to an account Desktop has signed in to before. A running Desktop is never quit automatically and stays on its account.",
        example: "Desktop is closed when auto switch moves Claude Code to B, so Desktop moves to B too. If Desktop is open, it stays on A.",
        topic: .autoSwitch)
    static let showBurnAnimals = SettingTip(
        summary: "Show an animated animal in the 5h percentage ring for how fast that window is being used.",
        detail: "The animal is the time until the window reaches 100% at the current pace: snail over 5h, turtle 3-5h, rabbit 1.5-3h, horse 30m-1.5h, cheetah under 30m, rocket at 5%/min held for 3 minutes or 10%/min. An account at cheetah or rocket refreshes every minute. It appears in the 5h percentage ring only, taking turns with the percentage, once two refreshes in a row show usage rising, and only while that 5h usage changed in the last 10 minutes. Off by default; sorting by burn rate works either way.",
        example: "5h goes from 20% to 40% in 10 minutes: at that pace 100% is 30 minutes away, so the gauge shows a horse.",
        topic: .readingCard)
    static let autoPing = SettingTip(
        summary: "Start idle usage windows automatically.",
        detail: "A window only starts counting at the first message. After each refresh, Auto Ping sends one short message from every active account whose 5h or 7d window is not running, so it resets sooner.",
        example: "An account idle since yesterday shows 5h with no reset time. Auto Ping starts it at 09:00, so it resets at 14:00 instead of five hours after you first use it.",
        topic: .ping)
    static let autoSwitch = SettingTip(
        summary: "Move Claude Code to another account before a limit blocks you.",
        detail: "When the account Claude Code uses reaches a threshold, Claude Code moves to the account whose weekly quota is closest to going unused. Order: high critical, then the home account (bolt) after its 5h reset, then critical, then the threshold switch. Only accounts already signed in to Claude Code are used.",
        example: "Claude Code is on A at 5h 95%. B has 5h 10% and 7d 40% with two days left, so Claude Code moves to B.",
        topic: .autoSwitch)
    static let fiveHourThreshold = SettingTip(
        summary: "5h usage at which auto switch moves Claude Code.",
        detail: "When the active account's 5h usage reaches this, auto switch picks another account.",
        example: "At 95%, an account at 5h 94% stays; at 95% Claude Code moves.",
        topic: .autoSwitch)
    static let sevenDayThreshold = SettingTip(
        summary: "7d usage at which auto switch moves Claude Code.",
        detail: "When the active account's weekly usage reaches this, auto switch picks another account, whatever its 5h usage.",
        example: "At 99%, an account at 5h 20% but 7d 99% still moves Claude Code away.",
        topic: .autoSwitch)
    static let menuBarToggle = SettingTip(
        summary: "Show the auto switch toggle in the menu bar popover near a limit.",
        detail: "When auto switch is off, the popover footer shows its toggle once the active account comes close to a threshold, so you can turn it on in time.",
        example: "Auto switch off, thresholds 95/99: at 5h 80% the toggle appears in the popover footer.",
        topic: .autoSwitch)
    static let criticalPace = SettingTip(
        summary: "Assumed use per 5h window, outside quiet hours, for the critical check.",
        detail: "An account is critical when, using this share of every 5h window from now until its weekly reset (quiet hours excluded), it still would not reach 100% of 7d. X (Dashboard header) converts 5h use into 7d use.",
        example: "X 16%, pace 50%: each window costs 8% of 7d. 7d at 60% with 30 active hours left (6 windows): 60 + 48 = 108, so not critical. 7d at 55% with 25 hours (5 windows): 55 + 40 = 95, so critical.",
        topic: .criticalAccounts)
    static let highCriticalPace = SettingTip(
        summary: "Assumed use per 5h window, around the clock, for the high critical check.",
        detail: "An account is high critical when even this heavier pace, with no quiet hours, would not reach 100% of 7d by the reset. High critical accounts win over the home account.",
        example: "X 16%, pace 80%: 12.8% of 7d per window. 7d at 60% with 10 hours left (2 windows): 60 + 25.6 = 85.6, so high critical.",
        topic: .criticalAccounts)
    static let quietHours = SettingTip(
        summary: "Daily hours not counted as usable for the critical check.",
        detail: "Hours between start and end each day are left out when counting the 5h windows still usable before a weekly reset. A start after the end crosses midnight. Equal start and end means no quiet hours. High critical ignores quiet hours.",
        example: "Quiet 00:00 to 07:00, reset in 48 hours from 10:00: two quiet blocks drop 14 hours, leaving 34 hours (6.8 windows).",
        topic: .criticalAccounts)
    static let timeZone = SettingTip(
        summary: "Time zone the quiet hours are read in.",
        detail: "Quiet hours follow this zone, not the Mac's. Useful when travelling or when your working day follows another zone.",
        example: "Zone Asia/Ho_Chi_Minh, quiet 00:00 to 07:00: on a Mac set to Paris time that is 19:00 to 02:00 in summer.",
        topic: .criticalAccounts)
    static let autoUpdate = SettingTip(
        summary: "Install new releases once a day.",
        detail: "Checks GitHub once a day and installs a newer version automatically. Check for Updates does the same on demand.",
        example: "A release published at noon is installed by the next daily check.",
        topic: .refreshUpdates)

    static let all: [SettingTip] = [
        autoRefresh, refreshInterval, autoPing, autoSwitch, fiveHourThreshold, sevenDayThreshold,
        menuBarToggle, criticalPace, highCriticalPace, quietHours, timeZone, autoUpdate,
    ]
}

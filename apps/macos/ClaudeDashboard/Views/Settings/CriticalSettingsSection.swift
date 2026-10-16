import SwiftUI

/// Settings > General > Critical Accounts: both paces, quiet hours and their time zone.
///
///     CriticalSettingsSection(settings: vm.criticalSettings, openHelp: { vm.openHelp($0) })
struct CriticalSettingsSection: View {
    @ObservedObject var settings: CriticalSettings
    let openHelp: (HelpTopic) -> Void

    var body: some View {
        Section {
            LabeledContent {
                Stepper("\(settings.pace)%", value: $settings.pace,
                        in: CriticalSettings.paceRange, step: CriticalSettings.paceStep)
            } label: { label("Critical pace", SettingsTips.criticalPace) }
            LabeledContent {
                Stepper("\(settings.highPace)%", value: $settings.highPace,
                        in: settings.pace...CriticalSettings.paceRange.upperBound, step: CriticalSettings.paceStep)
            } label: { label("High critical pace", SettingsTips.highCriticalPace) }
            LabeledContent {
                HStack(spacing: 6) {
                    hourPicker("Start", $settings.quietStart)
                    Text("to")
                    hourPicker("End", $settings.quietEnd)
                }
            } label: { label("Quiet hours", SettingsTips.quietHours) }
            LabeledContent {
                Picker("Time zone", selection: $settings.timeZoneId) {
                    ForEach(TimeZone.knownTimeZoneIdentifiers.sorted(), id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                .frame(maxWidth: 240)
            } label: { label("Time zone", SettingsTips.timeZone) }
        } header: {
            Text("Critical Accounts")
        } footer: {
            Text("Marks accounts whose weekly quota will go unused, and lets auto switch move Claude Code to them when their 5h window is free.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private func label(_ title: String, _ tip: SettingTip) -> some View {
        HStack(spacing: 4) {
            Text(title)
            InfoTip(tip) { openHelp(tip.topic) }
        }
    }

    private func hourPicker(_ title: String, _ value: Binding<Int>) -> some View {
        Picker(title, selection: value) {
            ForEach(CriticalSettings.hourRange, id: \.self) { Text(String(format: "%02d:00", $0)).tag($0) }
        }
        .labelsHidden()
        .frame(width: 84)
    }
}

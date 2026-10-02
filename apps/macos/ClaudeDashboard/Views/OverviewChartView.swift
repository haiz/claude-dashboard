import SwiftUI
import Charts

struct OverviewChartView: View {
    @ObservedObject var viewModel: DashboardViewModel

    @State private var selectedWindow: UsageWindow = .fiveHour
    @State private var visibleRange: ClosedRange<Date> = TimeRangePreset.fiveHour.range(endingAt: Date())
    @State private var selectedAccounts: Set<UUID> = []
    @State private var logs: [UsageLogEntry] = []
    @State private var loadTask: Task<Void, Never>?
    @State private var isLoading = false
    @State private var hasLoaded = false
    @State private var hoverDate: Date?
    @State private var hoverX: CGFloat = 0
    @State private var chartWidth: CGFloat = 1

    private static let lineColors: [Color] = [
        .orange, .cyan, .green, .purple, .pink, .blue, .yellow, .mint, .indigo, .red
    ]

    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: "Overview") {
                if isLoading {
                    ProgressView().controlSize(.small)
                }
            }

            Divider()

            // Interactive chart
            if logs.isEmpty && !hasLoaded {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if logs.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "chart.line.downtrend.xyaxis")
                        .font(.system(size: 36))
                        .foregroundStyle(.secondary)
                    Text("No data yet")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
                InteractiveChartContainer(
                    initialPreset: .fiveHour,
                    dataPoints: logs,
                    chartHeight: 300,
                    liveTick: viewModel.lastLogsUpdatedAt,
                    autoFollowsLiveEdge: true,
                    averageRateProvider: { range, _ in
                        weightedAverageRate(in: range)
                    },
                    onRangeChanged: { range in
                        visibleRange = range
                        loadLogs(range: range)
                    },
                    chartContent: { range in
                        overviewChart(range: range)
                    },
                    toolbarExtra: {
                        Picker("Window", selection: $selectedWindow) {
                            Text("5h").tag(UsageWindow.fiveHour)
                            Text("7d").tag(UsageWindow.sevenDay)
                            Text("F").tag(UsageWindow.fable)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .frame(width: 180)
                    }
                )
            }

            Divider()

            // Legend with toggles
            legendView
        }
        .onAppear { loadLogs() }
        .onDisappear { loadTask?.cancel() }
        .onChange(of: selectedWindow) { _ in loadLogs(range: visibleRange) }
        // While the chart is on screen, InteractiveChartContainer drives the per-refresh
        // reload (advancing the live window as needed). Only bootstrap the empty state here,
        // since the container isn't in the tree until there's data to show.
        .onChange(of: viewModel.lastLogsUpdatedAt) { _ in
            if logs.isEmpty { loadLogs() }
        }
    }

    // MARK: - Chart

    private func overviewChart(range: ClosedRange<Date>) -> some View {
        ZStack(alignment: hoverX > chartWidth / 2 ? .topLeading : .topTrailing) {
            Chart {
                // Per-account lines
                ForEach(viewModel.accountStates.filter { selectedAccounts.contains($0.id) }) { state in
                    let accountLogs = logs.filter { $0.accountId == state.id }
                    ForEach(accountLogs) { log in
                        LineMark(
                            x: .value("Time", log.recordedAt),
                            y: .value("Usage", log.utilization),
                            series: .value("Account", state.account.name)
                        )
                        .foregroundStyle(by: .value("Account", state.account.name))
                        .lineStyle(StrokeStyle(lineWidth: 1.5))
                        .interpolationMethod(.monotone)
                    }
                }

                // Limit markers
                ForEach(logs.filter { $0.isLimited && selectedAccounts.contains($0.accountId) }) { log in
                    PointMark(
                        x: .value("Time", log.recordedAt),
                        y: .value("Usage", log.utilization)
                    )
                    .foregroundStyle(.red)
                    .annotation(position: .top) {
                        Text("⚠").font(.caption2)
                    }
                }

                ForEach(noonMarkers(in: range), id: \.self) { noon in
                    RuleMark(x: .value("Noon", noon))
                        .foregroundStyle(.secondary.opacity(0.15))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [3, 4]))
                }

                // Hover vertical line
                if let hoverDate {
                    RuleMark(x: .value("Hover", hoverDate))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                }
            }
            .chartXScale(domain: range.lowerBound...range.upperBound)
            .chartXAxis {
                let marks = axisMarkDates(for: range)
                AxisMarks(values: marks) { value in
                    AxisGridLine()
                    if let date = value.as(Date.self) {
                        AxisValueLabel {
                            Text(axisLabel(for: date, in: range))
                                .font(.caption2)
                        }
                    }
                }
            }
            .chartForegroundStyleScale(domain: chartColorDomain, range: chartColorRange)
            .chartYScale(domain: 0...105)
            .chartYAxis {
                AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                    AxisGridLine()
                    AxisValueLabel {
                        if let v = value.as(Int.self) { Text("\(v)%") }
                    }
                }
            }
            .chartLegend(.hidden)
            .chartOverlay { proxy in
                GeometryReader { geo in
                    Rectangle().fill(.clear).contentShape(Rectangle())
                        .onContinuousHover { phase in
                            switch phase {
                            case .active(let location):
                                hoverDate = proxy.value(atX: location.x)
                                hoverX = location.x
                                chartWidth = geo.size.width
                            case .ended:
                                hoverDate = nil
                            }
                        }
                }
            }

            // Hover tooltip (auto-flips side based on cursor position)
            if let hoverDate {
                hoverTooltip(for: hoverDate)
                    .padding(8)
                    .allowsHitTesting(false)
            }
        }
    }

    // MARK: - Hover Tooltip

    @ViewBuilder
    private func hoverTooltip(for date: Date) -> some View {
        let visibleStates = viewModel.accountStates.filter { selectedAccounts.contains($0.id) }

        VStack(alignment: .leading, spacing: 3) {
            Text(formatHoverTime(date))
                .font(.caption2.bold())
                .foregroundStyle(.secondary)

            ForEach(Array(visibleStates.enumerated()), id: \.element.id) { _, state in
                let accountLogs = logs.filter { $0.accountId == state.id }
                    .sorted { $0.recordedAt < $1.recordedAt }
                let util = interpolate(at: date, in: accountLogs)
                let rate = computeRate(at: date, in: accountLogs)

                HStack(spacing: 4) {
                    Circle()
                        .fill(colorForAccount(state))
                        .frame(width: 6, height: 6)
                    Text(state.account.name)
                        .font(.caption2)
                        .lineLimit(1)
                    Spacer(minLength: 8)
                    if let util {
                        Text(String(format: "%.0f%%", util))
                            .font(.caption2.monospacedDigit())
                    }
                    if let rate {
                        Text(String(format: "%+.1f%%/h", rate))
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(rate > 0 ? .orange : .green)
                    } else {
                        Text("--")
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.tertiary)
                    }
                }
            }
        }
        .padding(8)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 6))
        .fixedSize()
    }

    private func formatHoverTime(_ date: Date) -> String {
        let df = DateFormatter()
        df.dateFormat = "d MMM HH:mm"
        return df.string(from: date)
    }

    private func computeRate(at date: Date, in sortedLogs: [UsageLogEntry]) -> Double? {
        guard sortedLogs.count >= 2 else { return nil }

        let before = sortedLogs.last(where: { $0.recordedAt <= date })
        let after = sortedLogs.first(where: { $0.recordedAt > date })

        if let b = before, let a = after {
            let dt = a.recordedAt.timeIntervalSince(b.recordedAt) / 3600
            guard dt > 0.01 else { return nil }
            return (a.utilization - b.utilization) / dt
        }

        // At edges, use nearest two consecutive points
        if before != nil, after == nil, sortedLogs.count >= 2 {
            let b = sortedLogs[sortedLogs.count - 2]
            let a = sortedLogs[sortedLogs.count - 1]
            let dt = a.recordedAt.timeIntervalSince(b.recordedAt) / 3600
            guard dt > 0.01 else { return nil }
            return (a.utilization - b.utilization) / dt
        }

        if before == nil, after != nil, sortedLogs.count >= 2 {
            let b = sortedLogs[0]
            let a = sortedLogs[1]
            let dt = a.recordedAt.timeIntervalSince(b.recordedAt) / 3600
            guard dt > 0.01 else { return nil }
            return (a.utilization - b.utilization) / dt
        }

        return nil
    }

    // MARK: - Colors

    private func colorForAccount(_ state: AccountUsageState) -> Color {
        guard let index = viewModel.accountStates.firstIndex(where: { $0.id == state.id }) else {
            return .blue
        }
        return Self.lineColors[index % Self.lineColors.count]
    }

    private var chartColorDomain: [String] {
        viewModel.accountStates.map { $0.account.name }
    }

    private var chartColorRange: [Color] {
        viewModel.accountStates.indices.map { Self.lineColors[$0 % Self.lineColors.count] }
    }

    // MARK: - Legend

    private var legendView: some View {
        ScrollView {
            VStack(spacing: 4) {
                ForEach(Array(viewModel.accountStates.enumerated()), id: \.element.id) { index, state in
                    let color = Self.lineColors[index % Self.lineColors.count]
                    let isSelected = selectedAccounts.contains(state.id)

                    Button {
                        if isSelected {
                            selectedAccounts.remove(state.id)
                        } else {
                            selectedAccounts.insert(state.id)
                        }
                        loadLogs(range: visibleRange)
                    } label: {
                        HStack {
                            Circle()
                                .fill(isSelected ? color : Color.secondary.opacity(0.3))
                                .frame(width: 8, height: 8)
                            Text(state.account.name)
                                .font(.caption)
                                .foregroundStyle(isSelected ? .primary : .secondary)
                            if let email = state.account.email, email != state.account.name {
                                Text(email)
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            Spacer()
                            if let animal = animalForSelectedWindow(state.burnRates) {
                                Text(animal)
                            } else {
                                Text("—")
                                    .foregroundStyle(.tertiary)
                            }
                        }
                        .padding(.horizontal)
                        .padding(.vertical, 4)
                    }
                    .buttonStyle(HoverableRowStyle(selected: isSelected))
                }
            }
        }
        .frame(maxHeight: .infinity)
    }

    // MARK: - Data Helpers

    /// Plan-weighted mean of each selected account's burn rate over `range`: the sum
    /// of its positive utilization deltas (resets are drops, so they are skipped) per
    /// hour. One pass over `logs`, which are sorted by `recordedAt`.
    private func weightedAverageRate(in range: ClosedRange<Date>) -> Double? {
        let totalHours = range.upperBound.timeIntervalSince(range.lowerBound) / 3600
        guard totalHours > 0.01 else { return nil }

        var lastUtilization: [UUID: Double] = [:]
        var positiveDeltas: [UUID: Double] = [:]
        for log in logs where selectedAccounts.contains(log.accountId) && range.contains(log.recordedAt) {
            if let previous = lastUtilization[log.accountId] {
                positiveDeltas[log.accountId, default: 0] += max(0, log.utilization - previous)
            }
            lastUtilization[log.accountId] = log.utilization
        }

        var weightedSum = 0.0
        var totalWeight = 0.0
        for state in viewModel.accountStates where positiveDeltas[state.id] != nil {
            let w = Self.planWeight(state.account.plan)
            weightedSum += positiveDeltas[state.id, default: 0] * w
            totalWeight += w
        }
        guard totalWeight > 0 else { return nil }
        return weightedSum / totalWeight / totalHours
    }

    private static func planWeight(_ plan: AccountPlan) -> Double {
        switch plan {
        case .pro: return 1
        case .max5x: return 5
        case .max20x: return 20
        case .max200: return 10
        }
    }

    private func interpolate(at time: Date, in logs: [UsageLogEntry]) -> Double? {
        guard !logs.isEmpty else { return nil }

        if let exact = logs.first(where: { $0.recordedAt == time }) {
            return exact.utilization
        }

        let before = logs.last(where: { $0.recordedAt <= time })
        let after = logs.first(where: { $0.recordedAt >= time })

        if let b = before, let a = after, b.recordedAt != a.recordedAt {
            let fraction = time.timeIntervalSince(b.recordedAt) / a.recordedAt.timeIntervalSince(b.recordedAt)
            return b.utilization + (a.utilization - b.utilization) * fraction
        }

        return before?.utilization ?? after?.utilization
    }

    private func animalForSelectedWindow(_ rates: BurnRates?) -> String? {
        switch selectedWindow {
        case .fiveHour: return rates?.fiveHour?.animal
        case .sevenDay: return rates?.sevenDay?.animal
        case .fable: return rates?.fable?.animal
        }
    }

    private func axisMarkDates(for range: ClosedRange<Date>) -> [Date] {
        let duration = range.upperBound.timeIntervalSince(range.lowerBound)
        let cal = Calendar.current
        var dates: [Date] = []
        if duration <= 6 * 3600 {
            var comps = cal.dateComponents([.year, .month, .day, .hour], from: range.lowerBound)
            comps.minute = 0
            var t = cal.date(from: comps)!
            if t < range.lowerBound { t = cal.date(byAdding: .hour, value: 1, to: t)! }
            while t <= range.upperBound {
                dates.append(t)
                t = cal.date(byAdding: .hour, value: 1, to: t)!
            }
        } else {
            let includeNoon = duration <= 7 * 86400
            var day = cal.startOfDay(for: range.lowerBound)
            while day <= range.upperBound {
                if day >= range.lowerBound { dates.append(day) }
                if includeNoon,
                   let noon = cal.date(bySettingHour: 12, minute: 0, second: 0, of: day),
                   noon >= range.lowerBound, noon <= range.upperBound {
                    dates.append(noon)
                }
                day = cal.date(byAdding: .day, value: 1, to: day)!
            }
        }
        return dates.sorted()
    }

    private func axisLabel(for date: Date, in range: ClosedRange<Date>) -> String {
        let duration = range.upperBound.timeIntervalSince(range.lowerBound)
        let df = DateFormatter()
        if duration <= 6 * 3600 {
            df.dateFormat = "ha"
            return df.string(from: date)
        }
        let hour = Calendar.current.component(.hour, from: date)
        df.dateFormat = hour == 0 ? "MMM d" : "MMM d ha"
        return df.string(from: date)
    }

    private func noonMarkers(in range: ClosedRange<Date>) -> [Date] {
        let cal = Calendar.current
        var result: [Date] = []
        var day = cal.startOfDay(for: range.lowerBound)
        while day <= range.upperBound {
            if let noon = cal.date(bySettingHour: 12, minute: 0, second: 0, of: day),
               range.contains(noon) {
                result.append(noon)
            }
            day = cal.date(byAdding: .day, value: 1, to: day)!
        }
        return result
    }

    /// Starts a load for `range` (or the current window slid to now), cancelling any
    /// load still in flight so a stale result never overwrites a newer one. The fetch
    /// runs off the main actor; the header spinner appears only if it takes > 200 ms,
    /// so quick reloads (live ticks, pan steps) do not flicker it.
    private func loadLogs(range: ClosedRange<Date>? = nil) {
        if selectedAccounts.isEmpty {
            selectedAccounts = Set(viewModel.accountStates.map(\.id))
        }

        let effectiveRange: ClosedRange<Date>
        if let range {
            effectiveRange = range
        } else {
            // Refresh to current time so we always include the latest data
            let duration = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound)
            let now = Date()
            visibleRange = now.addingTimeInterval(-duration)...now
            effectiveRange = visibleRange
        }

        let store = viewModel.logStore
        let window = selectedWindow
        let accountIds = viewModel.accountStates.map(\.id)

        loadTask?.cancel()
        loadTask = Task { @MainActor in
            let spinner = Task {
                try? await Task.sleep(nanoseconds: 200_000_000)
                if !Task.isCancelled { isLoading = true }
            }
            defer { spinner.cancel() }

            guard let fetched = await Self.fetchLogs(
                store: store, window: window, range: effectiveRange, accountIds: accountIds
            ) else { return }
            logs = fetched
            hasLoaded = true
            isLoading = false
        }
    }

    /// Fetches `range` plus the two points either side of it per account, so lines
    /// run to the chart edges. One indexed query per account: measured faster than
    /// `allLogs`, which scans the whole index. Runs off the main actor, as does the
    /// reset-transition pass. Returns nil once cancelled.
    nonisolated private static func fetchLogs(
        store: UsageLogStore,
        window: UsageWindow,
        range: ClosedRange<Date>,
        accountIds: [UUID]
    ) async -> [UsageLogEntry]? {
        var fetched: [UsageLogEntry] = []
        for accountId in accountIds {
            if Task.isCancelled { return nil }
            fetched += await store.logsBefore(accountId: accountId, window: window, before: range.lowerBound, limit: 2)
            fetched += await store.logs(accountId: accountId, window: window, from: range.lowerBound, to: range.upperBound)
            fetched += await store.logsAfter(accountId: accountId, window: window, after: range.upperBound, limit: 2)
        }
        if Task.isCancelled { return nil }
        return fetched.withResetTransitions()
    }
}

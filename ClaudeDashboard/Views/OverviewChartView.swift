import SwiftUI
import Charts

struct OverviewChartView: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onBack: () -> Void

    @State private var selectedWindow: UsageWindow = .fiveHour
    @State private var visibleRange: ClosedRange<Date> = {
        let now = Date()
        return now.addingTimeInterval(-86400)...now
    }()
    @State private var selectedAccounts: Set<UUID> = []
    @State private var logs: [UsageLogEntry] = []
    @State private var hoverDate: Date?
    @State private var hoverX: CGFloat = 0
    @State private var chartWidth: CGFloat = 1

    private static let totalColor = Color.white.opacity(0.85)

    private static let lineColors: [Color] = [
        .orange, .cyan, .green, .purple, .pink, .blue, .yellow, .mint, .indigo, .red
    ]

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button(action: onBack) {
                    Label("Back", systemImage: "chevron.left")
                }
                .buttonStyle(.borderless)

                Text("Overview")
                    .font(.title2.bold())

                Spacer()
            }
            .padding()

            Divider()

            // Interactive chart
            if logs.isEmpty {
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
                    initialPreset: .day,
                    dataPoints: logs,
                    chartHeight: 300,
                    averageRateProvider: { range, allLogs in
                        let totalPoints = computeTotalLine().filter {
                            $0.time >= range.lowerBound && $0.time <= range.upperBound
                        }
                        guard totalPoints.count >= 2 else { return nil }
                        let totalHours = range.upperBound.timeIntervalSince(range.lowerBound) / 3600
                        guard totalHours > 0.01 else { return nil }
                        var positiveDeltas = 0.0
                        for i in 1..<totalPoints.count {
                            let delta = totalPoints[i].value - totalPoints[i - 1].value
                            if delta > 0 { positiveDeltas += delta }
                        }
                        return positiveDeltas / totalHours
                    },
                    onRangeChanged: { range in
                        visibleRange = range
                        Task { await loadLogs(range: range) }
                    },
                    chartContent: {
                        overviewChart
                    },
                    toolbarExtra: {
                        Picker("Window", selection: $selectedWindow) {
                            Text("5h").tag(UsageWindow.fiveHour)
                            Text("7d").tag(UsageWindow.sevenDay)
                            Text("S").tag(UsageWindow.sonnet)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .frame(width: 140)
                    }
                )
            }

            Divider()

            // Legend with toggles
            legendView
        }
        .task { await loadLogs() }
        .onChange(of: selectedWindow) { _ in Task { await loadLogs() } }
    }

    // MARK: - Chart

    private var overviewChart: some View {
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

                // Total line
                ForEach(computeTotalLine(), id: \.time) { point in
                    LineMark(
                        x: .value("Time", point.time),
                        y: .value("Usage", point.value),
                        series: .value("Account", "Total")
                    )
                    .foregroundStyle(by: .value("Account", "Total"))
                    .lineStyle(StrokeStyle(lineWidth: 3, dash: [6, 3]))
                    .interpolationMethod(.monotone)
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

                // Hover vertical line
                if let hoverDate {
                    RuleMark(x: .value("Hover", hoverDate))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
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
        viewModel.accountStates.map { $0.account.name } + ["Total"]
    }

    private var chartColorRange: [Color] {
        viewModel.accountStates.indices.map { Self.lineColors[$0 % Self.lineColors.count] } + [Self.totalColor]
    }

    // MARK: - Legend

    private var legendView: some View {
        ScrollView {
            VStack(spacing: 4) {
                // Total row
                HStack {
                    Circle()
                        .fill(Self.totalColor)
                        .frame(width: 8, height: 8)
                    Text("Total")
                        .font(.caption.bold())
                    Text("(dashed)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
                .padding(.horizontal)
                .padding(.vertical, 4)

                ForEach(Array(viewModel.accountStates.enumerated()), id: \.element.id) { index, state in
                    let color = Self.lineColors[index % Self.lineColors.count]
                    let isSelected = selectedAccounts.contains(state.id)

                    Button {
                        if isSelected {
                            selectedAccounts.remove(state.id)
                        } else {
                            selectedAccounts.insert(state.id)
                        }
                        Task { await loadLogs() }
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
                    .buttonStyle(.plain)
                }
            }
        }
        .frame(maxHeight: 120)
    }

    // MARK: - Data Helpers

    struct TotalPoint: Identifiable {
        let time: Date
        let value: Double
        var id: Date { time }
    }

    private func computeTotalLine() -> [TotalPoint] {
        let selected = viewModel.accountStates.filter { selectedAccounts.contains($0.id) }
        guard !selected.isEmpty else { return [] }

        let selectedLogs = logs.filter { selectedAccounts.contains($0.accountId) }
        let allTimes = Set(selectedLogs.map { $0.recordedAt }).sorted()

        let weights: [UUID: Double] = Dictionary(uniqueKeysWithValues: selected.map { state in
            let w: Double
            switch state.account.plan {
            case .pro: w = 1
            case .max5x: w = 5
            case .max20x: w = 20
            case .max200: w = 10
            }
            return (state.id, w)
        })

        return allTimes.map { time in
            var weightedSum = 0.0
            var totalWeight = 0.0

            for state in selected {
                let accountLogs = selectedLogs.filter { $0.accountId == state.id }
                if let utilization = interpolate(at: time, in: accountLogs) {
                    let w = weights[state.id] ?? 1
                    weightedSum += utilization * w
                    totalWeight += w
                }
            }

            let avg = totalWeight > 0 ? weightedSum / totalWeight : 0
            return TotalPoint(time: time, value: avg)
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
        case .sonnet: return rates?.sonnet?.animal
        }
    }

    private func loadLogs(range: ClosedRange<Date>? = nil) async {
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
        logs = await store.allLogs(window: selectedWindow, from: effectiveRange.lowerBound, to: effectiveRange.upperBound)
    }
}

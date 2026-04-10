import SwiftUI
import Charts

struct OverviewChartView: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onBack: () -> Void

    @State private var selectedWindow: UsageWindow = .fiveHour
    @State private var timeRange: TimeRange = .day
    @State private var selectedAccounts: Set<UUID> = []
    @State private var logs: [UsageLogEntry] = []

    enum TimeRange: String, CaseIterable {
        case day = "24h"
        case threeDay = "3d"
        case week = "7d"
        case month = "30d"

        var seconds: TimeInterval {
            switch self {
            case .day: return 86400
            case .threeDay: return 3 * 86400
            case .week: return 7 * 86400
            case .month: return 30 * 86400
            }
        }
    }

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

            // Controls
            HStack {
                Picker("Window", selection: $selectedWindow) {
                    Text("5h").tag(UsageWindow.fiveHour)
                    Text("7d").tag(UsageWindow.sevenDay)
                    Text("S").tag(UsageWindow.sonnet)
                }
                .pickerStyle(.segmented)
                .frame(width: 180)

                Spacer()

                Picker("Time", selection: $timeRange) {
                    ForEach(TimeRange.allCases, id: \.self) { range in
                        Text(range.rawValue).tag(range)
                    }
                }
                .frame(width: 100)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)

            // Chart
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
                overviewChart
                    .padding()
            }

            Divider()

            // Legend with toggles
            legendView
        }
        .task { await loadLogs() }
        .onChange(of: selectedWindow) { _ in Task { await loadLogs() } }
        .onChange(of: timeRange) { _ in Task { await loadLogs() } }
    }

    private var overviewChart: some View {
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
                .foregroundStyle(.primary)
                .lineStyle(StrokeStyle(lineWidth: 3))
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
        }
        .chartYScale(domain: 0...105)
        .chartYAxis {
            AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let v = value.as(Int.self) { Text("\(v)%") }
                }
            }
        }
        .frame(height: 300)
    }

    private var legendView: some View {
        ScrollView {
            VStack(spacing: 4) {
                // Total row
                HStack {
                    Image(systemName: "checkmark.square.fill")
                        .foregroundStyle(.primary)
                    Text("Total")
                        .font(.caption.bold())
                    Spacer()
                }
                .padding(.horizontal)
                .padding(.vertical, 4)

                ForEach(viewModel.accountStates) { state in
                    Button {
                        if selectedAccounts.contains(state.id) {
                            selectedAccounts.remove(state.id)
                        } else {
                            selectedAccounts.insert(state.id)
                        }
                        Task { await loadLogs() }
                    } label: {
                        HStack {
                            Image(systemName: selectedAccounts.contains(state.id) ? "checkmark.square.fill" : "square")
                                .foregroundStyle(selectedAccounts.contains(state.id) ? Color.accentColor : Color.secondary)
                            Text(state.account.name)
                                .font(.caption)
                            if let email = state.account.email, email != state.account.name {
                                Text(email)
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            Spacer()
                            if let animal = state.burnRates?.fiveHour?.animal {
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

    private func loadLogs() async {
        if selectedAccounts.isEmpty {
            selectedAccounts = Set(viewModel.accountStates.map(\.id))
        }

        let from = Date().addingTimeInterval(-timeRange.seconds)
        let store = viewModel.logStore
        logs = await store.allLogs(window: selectedWindow, from: from, to: nil)
    }
}

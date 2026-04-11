import SwiftUI
import Charts

struct AccountDetailView: View {
    @StateObject var viewModel: AccountDetailViewModel
    let onBack: () -> Void

    @State private var hoverDate: Date?
    @State private var hoverX: CGFloat = 0
    @State private var chartWidth: CGFloat = 1
    @State private var cyclesExpanded = false

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button(action: onBack) {
                    Label("Back", systemImage: "chevron.left")
                }
                .buttonStyle(.borderless)

                Text(viewModel.accountName)
                    .font(.title2.bold())

                Spacer()

                Text(viewModel.accountPlan.rawValue)
                    .font(.caption2.bold())
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(viewModel.accountPlan.badgeColor.opacity(0.15))
                    .clipShape(Capsule())
            }
            .padding()

            Divider()

            // Interactive chart
            if viewModel.logs.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "chart.line.downtrend.xyaxis")
                        .font(.system(size: 36))
                        .foregroundStyle(.secondary)
                    Text("No data yet")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("Data will appear after the next refresh.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
                InteractiveChartContainer(
                    initialPreset: .day,
                    dataPoints: viewModel.logs,
                    chartHeight: 250,
                    onRangeChanged: { range in
                        viewModel.updateRange(range)
                    },
                    chartContent: {
                        usageChart
                    },
                    toolbarExtra: {
                        Picker("Window", selection: Binding(
                            get: { viewModel.selectedWindow },
                            set: { viewModel.selectWindow($0) }
                        )) {
                            Text("5h").tag(UsageWindow.fiveHour)
                            Text("7d").tag(UsageWindow.sevenDay)
                            Text("S").tag(UsageWindow.sonnet)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .frame(width: 140)

                        if viewModel.selectedCycle != nil {
                            Button("Show All") {
                                viewModel.selectCycle(nil)
                            }
                            .buttonStyle(.borderless)
                            .font(.caption)
                        }
                    }
                )
            }

            // Reset cycles list
            if !viewModel.resetCycles.isEmpty {
                Divider()
                resetCyclesList
            }
        }
        .task { await viewModel.loadData() }
    }

    private var usageChart: some View {
        ZStack(alignment: hoverX > chartWidth / 2 ? .topLeading : .topTrailing) {
            Chart {
                ForEach(viewModel.logs) { log in
                    LineMark(
                        x: .value("Time", log.recordedAt),
                        y: .value("Usage", log.utilization)
                    )
                    .foregroundStyle(Color.blue)
                    .interpolationMethod(.monotone)

                    if log.isLimited {
                        PointMark(
                            x: .value("Time", log.recordedAt),
                            y: .value("Usage", log.utilization)
                        )
                        .foregroundStyle(Color.red)
                        .annotation(position: .top) {
                            Text("⚠")
                                .font(.caption2)
                        }
                    }
                }

                RuleMark(y: .value("Limit", 100))
                    .foregroundStyle(.red.opacity(0.3))
                    .lineStyle(StrokeStyle(dash: [5, 5]))

                // Hover vertical line
                if let hoverDate {
                    RuleMark(x: .value("Hover", hoverDate))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                }
            }
            .chartYScale(domain: 0...105)
            .chartYAxis {
                AxisMarks(values: [0, 25, 50, 75, 100]) { value in
                    AxisGridLine()
                    AxisValueLabel {
                        if let v = value.as(Int.self) {
                            Text("\(v)%")
                        }
                    }
                }
            }
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
        let sortedLogs = viewModel.logs.sorted { $0.recordedAt < $1.recordedAt }
        let util = interpolateValue(at: date, in: sortedLogs)
        let rate = computeRate(at: date, in: sortedLogs)

        VStack(alignment: .leading, spacing: 3) {
            Text(formatHoverTime(date))
                .font(.caption2.bold())
                .foregroundStyle(.secondary)

            HStack(spacing: 6) {
                if let util {
                    Text(String(format: "%.0f%%", util))
                        .font(.caption.monospacedDigit().bold())
                }
                if let rate {
                    Text(String(format: "%+.1f%%/h", rate))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(rate > 0 ? .orange : .green)
                } else {
                    Text("--")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.tertiary)
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

    private func interpolateValue(at time: Date, in logs: [UsageLogEntry]) -> Double? {
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

    private func computeRate(at date: Date, in sortedLogs: [UsageLogEntry]) -> Double? {
        guard sortedLogs.count >= 2 else { return nil }

        let before = sortedLogs.last(where: { $0.recordedAt <= date })
        let after = sortedLogs.first(where: { $0.recordedAt > date })

        if let b = before, let a = after {
            let dt = a.recordedAt.timeIntervalSince(b.recordedAt) / 3600
            guard dt > 0.01 else { return nil }
            return (a.utilization - b.utilization) / dt
        }

        // At edges
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

    // MARK: - Reset Cycles

    private var resetCyclesList: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Clickable header
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    cyclesExpanded.toggle()
                }
            } label: {
                HStack {
                    Text("Reset Cycles")
                        .font(.caption.bold())
                        .foregroundStyle(.secondary)
                    Spacer()
                    Image(systemName: cyclesExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            // Collapsible content
            if cyclesExpanded {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(viewModel.resetCycles) { cycle in
                            Button {
                                viewModel.selectCycle(cycle)
                            } label: {
                                HStack {
                                    Circle()
                                        .fill(DashboardViewModel.usageColor(for: cycle.peakUtilization))
                                        .frame(width: 8, height: 8)
                                    Text(formatCycleRange(cycle))
                                        .font(.caption)
                                    Spacer()
                                    Text("peak: \(Int(cycle.peakUtilization))%")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    Text("\(cycle.dataPointCount) pts")
                                        .font(.caption2)
                                        .foregroundStyle(.tertiary)
                                }
                                .padding(.horizontal)
                                .padding(.vertical, 6)
                                .background(
                                    viewModel.selectedCycle?.resetsAt == cycle.resetsAt
                                        ? Color.accentColor.opacity(0.1)
                                        : Color.clear
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                .frame(maxHeight: 150)
            }
        }
    }

    private func formatCycleRange(_ cycle: ResetCycle) -> String {
        let df = DateFormatter()
        df.dateFormat = "d MMM HH:mm"
        return "\(df.string(from: cycle.firstRecordedAt)) – \(df.string(from: cycle.resetsAt))"
    }
}

import SwiftUI
import Charts

struct AccountDetailView: View {
    @StateObject var viewModel: AccountDetailViewModel
    let onBack: () -> Void

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

            // Window picker
            HStack {
                Picker("Window", selection: Binding(
                    get: { viewModel.selectedWindow },
                    set: { viewModel.selectWindow($0) }
                )) {
                    Text("5h").tag(UsageWindow.fiveHour)
                    Text("7d").tag(UsageWindow.sevenDay)
                    Text("S").tag(UsageWindow.sonnet)
                }
                .pickerStyle(.segmented)
                .frame(width: 180)

                Spacer()

                if viewModel.selectedCycle != nil {
                    Button("Show All") {
                        viewModel.selectCycle(nil)
                    }
                    .buttonStyle(.borderless)
                    .font(.caption)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 8)

            // Chart
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
                usageChart
                    .padding()
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
        .frame(height: 250)
    }

    private var resetCyclesList: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Reset Cycles")
                .font(.caption.bold())
                .foregroundStyle(.secondary)
                .padding(.horizontal)
                .padding(.top, 8)

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

    private func formatCycleRange(_ cycle: ResetCycle) -> String {
        let df = DateFormatter()
        df.dateFormat = "d MMM HH:mm"
        return "\(df.string(from: cycle.firstRecordedAt)) – \(df.string(from: cycle.resetsAt))"
    }
}

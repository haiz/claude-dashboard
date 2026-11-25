// ClaudeDashboard/Views/OverviewSpeedChart.swift
import SwiftUI
import Charts

/// The Overview Speed chart's data: one Max-equivalent %/h line per shown account, their
/// Total, and the y-axis top. Pure, so it is tested without a view.
///
///     let lines = OverviewSpeedLines.make(logs: logs, plans: plans, order: ids, selected: shown,
///                                         step: .six, range: range, now: Date(),
///                                         utcOffset: TimeInterval(TimeZone.current.secondsFromGMT(for: range.upperBound)))
struct OverviewSpeedLines: Equatable {
    struct AccountLine: Equatable {
        let id: UUID
        let points: [SpeedPoint]
    }

    let accounts: [AccountLine]
    let total: [SpeedPoint]
    let yMax: Double

    static let empty = OverviewSpeedLines(accounts: [], total: [], yMax: minimumYMax)
    private static let minimumYMax: Double = 10

    /// `logs` hold every account's entries (reset transitions and out-of-range buffer points
    /// included). Lines follow `order`; accounts outside `selected`, without logs or without a
    /// plan are skipped. `range` must be finite.
    static func make(logs: [UsageLogEntry], plans: [UUID: AccountPlan], order: [UUID],
                     selected: Set<UUID>, step: SpeedStep, range: ClosedRange<Date>,
                     now: Date, utcOffset: TimeInterval) -> OverviewSpeedLines {
        let byAccount = Dictionary(grouping: logs.lazy.filter { selected.contains($0.accountId) },
                                   by: \.accountId)
        var accounts: [AccountLine] = []
        var inputs: [SpeedSeries.AccountInput] = []
        for id in order where selected.contains(id) {
            guard let plan = plans[id], let group = byAccount[id], !group.isEmpty else { continue }
            let entries = group.sorted { $0.recordedAt < $1.recordedAt }
            let points = SpeedSeries.account(entries: entries, step: step.seconds, utcOffset: utcOffset,
                                             range: range, factor: PlanWeight.maxFactor(plan))
            accounts.append(AccountLine(id: id, points: points))
            inputs.append(SpeedSeries.AccountInput(points: points, lastEntry: entries[entries.count - 1].recordedAt))
        }
        let total = SpeedSeries.total(inputs, now: now)
        let maxValue = (accounts.flatMap(\.points) + total).map(\.value).max() ?? 0
        let yMax = max(minimumYMax, (maxValue * 1.1 / 5).rounded(.up) * 5)
        return OverviewSpeedLines(accounts: accounts, total: total, yMax: yMax)
    }
}

/// The Overview Speed chart: one thin line per account in its legend color and a bold Total,
/// in Max-equivalent %/h per `step` bucket, stamped at bucket ends. No limit markers.
struct OverviewSpeedChart: View {
    let lines: OverviewSpeedLines
    let range: ClosedRange<Date>
    let step: SpeedStep
    let names: [UUID: String]
    let colors: [UUID: Color]

    @State private var hoverDate: Date?
    @State private var hoverX: CGFloat = 0
    @State private var chartWidth: CGFloat = 1

    private static let totalSeries = "Total"

    var body: some View {
        ZStack(alignment: hoverX > chartWidth / 2 ? .topLeading : .topTrailing) {
            Chart {
                ForEach(lines.accounts, id: \.id) { line in
                    ForEach(line.points, id: \.end) { point in
                        LineMark(
                            x: .value("Time", point.end),
                            y: .value("Speed", point.value),
                            series: .value("Account", line.id.uuidString)
                        )
                        .foregroundStyle(colors[line.id] ?? .blue)
                        .lineStyle(StrokeStyle(lineWidth: 1.5))
                    }
                }

                ForEach(lines.total, id: \.end) { point in
                    LineMark(
                        x: .value("Time", point.end),
                        y: .value("Speed", point.value),
                        series: .value("Account", Self.totalSeries)
                    )
                    .foregroundStyle(Color.primary)
                    .lineStyle(StrokeStyle(lineWidth: 3))
                }

                ForEach(OverviewChartView.noonMarkers(in: range), id: \.self) { noon in
                    RuleMark(x: .value("Noon", noon))
                        .foregroundStyle(.secondary.opacity(0.15))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [3, 4]))
                }

                if let hoverDate {
                    RuleMark(x: .value("Hover", hoverDate))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                }
            }
            .chartXScale(domain: range.lowerBound...range.upperBound)
            .chartXAxis {
                AxisMarks(values: OverviewChartView.axisMarkDates(for: range)) { value in
                    AxisGridLine()
                    if let date = value.as(Date.self) {
                        AxisValueLabel {
                            Text(OverviewChartView.axisLabel(for: date, in: range))
                                .font(.caption2)
                        }
                    }
                }
            }
            .chartYScale(domain: 0...lines.yMax)
            .chartYAxis {
                AxisMarks { value in
                    AxisGridLine()
                    AxisValueLabel {
                        if let v = value.as(Double.self) { Text(String(format: "%.1f", v)) }
                    }
                }
            }
            .chartYAxisLabel("%/h (Max)")
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

            // Hover tooltip (flips side like the Usage tooltip)
            if let hoverDate {
                hoverTooltip(for: hoverDate)
                    .padding(8)
                    .allowsHitTesting(false)
            }
        }
    }

    // MARK: - Hover Tooltip

    /// The hovered bucket is the one with `end - step < hover <= end`, on the local clock.
    private func bucketEnd(for date: Date) -> Date {
        let offset = TimeInterval(TimeZone.current.secondsFromGMT(for: range.upperBound))
        let s = step.seconds
        let end = ((date.timeIntervalSince1970 + offset) / s).rounded(.up) * s - offset
        return Date(timeIntervalSince1970: end)
    }

    private func value(at end: Date, in points: [SpeedPoint]) -> Double? {
        points.first(where: { $0.end == end })?.value
    }

    private func hoverTooltip(for date: Date) -> some View {
        let end = bucketEnd(for: date)
        let df = DateFormatter()
        df.dateFormat = "HH:mm"
        // The day is unknown on 3d/7d/30d ranges, so name it unless the bucket ends today.
        let day = Calendar.current.isDateInToday(end) ? "" : { () -> String in
            let d = DateFormatter()
            d.dateFormat = "d MMM "
            return d.string(from: end)
        }()
        let header = day + df.string(from: end.addingTimeInterval(-step.seconds)) + "–" + df.string(from: end)

        return VStack(alignment: .leading, spacing: 3) {
            Text(header)
                .font(.caption2.bold())
                .foregroundStyle(.secondary)

            ForEach(lines.accounts, id: \.id) { line in
                tooltipRow(name: names[line.id] ?? "", color: colors[line.id] ?? .blue,
                           value: value(at: end, in: line.points), bold: false)
            }
            tooltipRow(name: Self.totalSeries, color: .primary,
                       value: value(at: end, in: lines.total), bold: true)
        }
        .padding(8)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 6))
        .fixedSize()
    }

    private func tooltipRow(name: String, color: Color, value: Double?, bold: Bool) -> some View {
        HStack(spacing: 4) {
            Circle()
                .fill(color)
                .frame(width: 6, height: 6)
            Text(name)
                .font(bold ? .caption2.bold() : .caption2)
                .lineLimit(1)
            Spacer(minLength: 8)
            if let value {
                Text(String(format: "%.1f", value))
                    .font(.caption2.monospacedDigit())
            } else {
                Text("--")
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.tertiary)
            }
        }
    }
}

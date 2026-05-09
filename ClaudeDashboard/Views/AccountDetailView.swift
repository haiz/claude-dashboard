import SwiftUI
import Charts

struct AccountDetailView: View {
    @StateObject var viewModel: AccountDetailViewModel
    @ObservedObject var dashboardViewModel: DashboardViewModel
    let onBack: () -> Void
    let onAllAccounts: () -> Void

    @State private var hoverDate: Date?
    @State private var hoverX: CGFloat = 0
    @State private var chartWidth: CGFloat = 1
    @State private var cyclesExpanded = false

    // Measure tool
    @State private var measureToolActive = false
    @State private var measureSelection: MeasureSelection? = nil
    @State private var measureHoverY: Double? = nil
    @State private var crosshairPushed = false

    private struct MeasurePoint: Equatable {
        let date: Date
        let utilization: Double
    }
    private struct MeasureSelection: Equatable {
        var pointA: MeasurePoint
        var pointB: MeasurePoint?
    }

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button(action: onBack) {
                    Label("Back", systemImage: "chevron.left")
                }
                .buttonStyle(HoverableButtonStyle(prominent: true))

                Button(action: onAllAccounts) {
                    Label("All accounts", systemImage: "rectangle.grid.2x2")
                }
                .buttonStyle(HoverableButtonStyle(prominent: true))

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
                    isInteractionDisabled: measureToolActive,
                    onRangeChanged: { range in
                        viewModel.updateRange(range)
                    },
                    chartContent: { range in
                        usageChart(range: range)
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
                            .buttonStyle(HoverableButtonStyle(verticalPadding: 3))
                            .font(.caption)
                        }

                        Divider().frame(height: 16)

                        Button {
                            measureToolActive.toggle()
                            if !measureToolActive {
                                measureSelection = nil
                                measureHoverY = nil
                                hoverDate = nil
                                if crosshairPushed {
                                    NSCursor.pop()
                                    crosshairPushed = false
                                }
                            }
                        } label: {
                            Image(systemName: measureToolActive ? "ruler.fill" : "ruler")
                                .frame(width: 20, height: 20)
                                .foregroundStyle(measureToolActive ? Color.accentColor : .primary)
                        }
                        .buttonStyle(.borderless)
                        .help(measureToolActive ? "Turn off measure tool" : "Measure tool: click 2 points to see Δ%")
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
        .onChange(of: dashboardViewModel.lastLogsUpdatedAt) { _ in
            Task { await viewModel.loadData(keepRange: true) }
        }
        .onChange(of: viewModel.selectedWindow) { _ in measureSelection = nil }
        .onChange(of: viewModel.selectedCycle?.resetsAt) { _ in measureSelection = nil }
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

    private var chartDayBands: [(index: Int, start: Date, end: Date)] {
        guard let first = viewModel.logs.map(\.recordedAt).min(),
              let last = viewModel.logs.map(\.recordedAt).max() else { return [] }
        let cal = Calendar.current
        var bands: [(start: Date, end: Date)] = []
        var day = cal.startOfDay(for: first)
        while day < last {
            let next = cal.date(byAdding: .day, value: 1, to: day)!
            bands.append((start: day, end: next))
            day = next
        }
        return bands.enumerated().map { (index: $0.offset, start: $0.element.start, end: $0.element.end) }
    }

    private func usageChart(range: ClosedRange<Date>) -> some View {
        let labelAlignment: Alignment = hoverX > chartWidth / 2 ? .topLeading : .topTrailing

        return ZStack(alignment: labelAlignment) {
            Chart {
                ForEach(chartDayBands, id: \.index) { band in
                    if band.index % 2 == 1 {
                        RectangleMark(
                            xStart: .value("DayStart", band.start),
                            xEnd: .value("DayEnd", band.end)
                        )
                        .foregroundStyle(.primary.opacity(0.04))
                    }
                }

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

                ForEach(noonMarkers(in: range), id: \.self) { noon in
                    RuleMark(x: .value("Noon", noon))
                        .foregroundStyle(.secondary.opacity(0.15))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [3, 4]))
                }

                RuleMark(y: .value("Limit", 100))
                    .foregroundStyle(.red.opacity(0.3))
                    .lineStyle(StrokeStyle(dash: [5, 5]))

                if viewModel.selectedWindow == .sevenDay {
                    ForEach(viewModel.fiveHourResetMarkers, id: \.self) { resetTime in
                        RuleMark(x: .value("5h reset", resetTime))
                            .foregroundStyle(.cyan.opacity(0.4))
                            .lineStyle(StrokeStyle(lineWidth: 1, dash: [2, 4]))
                            .annotation(position: .top, alignment: .center) {
                                Text("↺")
                                    .font(.system(size: 8))
                                    .foregroundStyle(.cyan.opacity(0.7))
                            }
                    }
                }

                // Hover vertical line (hidden after selection complete)
                if let hoverDate, measureSelection?.pointB == nil {
                    RuleMark(x: .value("Hover", hoverDate))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                }

                // Measure tool: horizontal crosshair (armed, no B yet)
                if measureToolActive, measureSelection?.pointB == nil, let y = measureHoverY {
                    RuleMark(y: .value("CursorY", y))
                        .foregroundStyle(.secondary.opacity(0.5))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
                }

                // Measure tool: selection band and endpoints
                if let sel = measureSelection {
                    RuleMark(x: .value("MeasureA", sel.pointA.date))
                        .foregroundStyle(Color.accentColor.opacity(0.8))
                        .lineStyle(StrokeStyle(lineWidth: 1))

                    if let b = sel.pointB {
                        RuleMark(x: .value("MeasureB", b.date))
                            .foregroundStyle(Color.accentColor.opacity(0.8))
                            .lineStyle(StrokeStyle(lineWidth: 1))

                        RectangleMark(
                            xStart: .value("BandStart", min(sel.pointA.date, b.date)),
                            xEnd: .value("BandEnd", max(sel.pointA.date, b.date))
                        )
                        .foregroundStyle(Color.accentColor.opacity(0.12))
                    }
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
                                if measureToolActive {
                                    measureHoverY = proxy.value(atY: location.y, as: Double.self)
                                    if !crosshairPushed {
                                        NSCursor.crosshair.push()
                                        crosshairPushed = true
                                    }
                                }
                            case .ended:
                                hoverDate = nil
                                measureHoverY = nil
                                if crosshairPushed {
                                    NSCursor.pop()
                                    crosshairPushed = false
                                }
                            }
                        }
                        .gesture(
                            SpatialTapGesture(count: 1, coordinateSpace: .local)
                                .onEnded { value in
                                    guard measureToolActive else { return }
                                    handleMeasureTap(at: value.location, proxy: proxy)
                                }
                        )
                }
            }

            // Hover tooltip (hidden when measure band is showing)
            if let hoverDate, measureSelection?.pointB == nil {
                hoverTooltip(for: hoverDate)
                    .padding(8)
                    .allowsHitTesting(false)
            }

            // Measure summary card
            if let sel = measureSelection, let b = sel.pointB {
                measureSummaryCard(a: sel.pointA, b: b)
                    .padding(8)
                    .allowsHitTesting(false)
            }
        }
    }

    private func handleMeasureTap(at location: CGPoint, proxy: ChartProxy) {
        guard let date: Date = proxy.value(atX: location.x) else { return }
        let sortedLogs = viewModel.logs.sorted { $0.recordedAt < $1.recordedAt }
        guard let util = interpolateValue(at: date, in: sortedLogs) else { return }
        let p = MeasurePoint(date: date, utilization: util)

        switch measureSelection {
        case .none:
            measureSelection = MeasureSelection(pointA: p, pointB: nil)
        case .some(let sel) where sel.pointB == nil:
            guard abs(p.date.timeIntervalSince(sel.pointA.date)) > 1 else { return }
            measureSelection = MeasureSelection(pointA: sel.pointA, pointB: p)
        case .some:
            measureSelection = nil
        }
    }

    @ViewBuilder
    private func measureSummaryCard(a: MeasurePoint, b: MeasurePoint) -> some View {
        let (start, end) = a.date <= b.date ? (a, b) : (b, a)
        let delta = end.utilization - start.utilization
        let dt = end.date.timeIntervalSince(start.date)

        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Text(formatHoverTime(start.date))
                Text("→")
                Text(formatHoverTime(end.date))
            }
            .font(.caption2)
            .foregroundStyle(.secondary)

            Text(String(format: "%+.0f%%: %.0f%% → %.0f%%", delta, start.utilization, end.utilization))
                .font(.caption.monospacedDigit().bold())
                .foregroundStyle(delta >= 0 ? Color.orange : Color.green)

            Text(formatDuration(dt))
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .padding(8)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 6))
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let h = Int(seconds) / 3600
        let m = (Int(seconds) % 3600) / 60
        if h > 0 { return "\(h)h \(m)m" }
        return "\(m)m"
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
            }
            .buttonStyle(HoverableRowStyle())

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
                            }
                            .buttonStyle(HoverableRowStyle(selected: viewModel.selectedCycle?.resetsAt == cycle.resetsAt))
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

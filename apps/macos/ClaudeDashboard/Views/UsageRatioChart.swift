import SwiftUI
import Charts

/// Dashboard header button showing X; click opens its history chart.
///
///     UsageRatioButton(controller: vm.ratioController)
struct UsageRatioButton: View {
    @ObservedObject var controller: UsageRatioController
    @State private var showingChart = false

    var body: some View {
        Button { showingChart = true } label: {
            Text(Self.label(controller.current))
                .monospacedDigit()
                .foregroundStyle(controller.current.isDefault ? Color.secondary : Color.primary)
        }
        .help(Self.help(controller.current))
        .popover(isPresented: $showingChart, arrowEdge: .bottom) {
            UsageRatioChart(controller: controller)
                .frame(width: 440, height: 280)
                .padding(16)
        }
    }

    static func label(_ value: UsageRatioValue) -> String {
        value.isDefault ? "X \(Int(value.x.rounded()))% (default)" : String(format: "X %.1f%%", value.x)
    }

    static func help(_ value: UsageRatioValue) -> String {
        let head = String(format: "100%% of a 5h window costs about %.1f%% of the 7d quota.", value.x)
        guard let updatedAt = value.updatedAt else {
            return head + " Default value: not enough usage history yet. Click for the history."
        }
        let time = updatedAt.formatted(date: .omitted, time: .shortened)
        return head + " Measured over the last 3 days, updated hourly (last at \(time)). Click for the history."
    }
}

/// Line chart of X over 7 days, 30 days or everything kept (90 days).
struct UsageRatioChart: View {
    let controller: UsageRatioController
    @State private var range: Range = .week
    @State private var points: [UsageRatioPoint] = []
    @State private var hovered: UsageRatioPoint?

    enum Range: String, CaseIterable, Identifiable {
        case week = "7 days", month = "30 days", all = "All"
        var id: Self { self }
        var seconds: TimeInterval? {
            switch self {
            case .week: return 7 * 86_400
            case .month: return 30 * 86_400
            case .all: return nil
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("X over time").font(.headline)
                Spacer()
                Picker("Range", selection: $range) {
                    ForEach(Range.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 220)
            }
            if points.isEmpty {
                Text("No history yet")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                chart
                Text(hovered.map { String(format: "%@: X %.1f%%", $0.date.formatted(date: .abbreviated, time: .shortened), $0.x) } ?? " ")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .task(id: range) {
            points = await controller.history(from: range.seconds.map { Date().addingTimeInterval(-$0) })
        }
    }

    private var chart: some View {
        Chart {
            ForEach(points, id: \.date) { p in
                LineMark(x: .value("Time", p.date), y: .value("X", p.x))
            }
            if let hovered {
                PointMark(x: .value("Time", hovered.date), y: .value("X", hovered.x))
            }
        }
        .chartYAxisLabel("% of 7d per full 5h")
        .chartOverlay { proxy in
            GeometryReader { geo in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onContinuousHover { phase in
                        switch phase {
                        case .active(let location):
                            let x = location.x - geo[proxy.plotAreaFrame].origin.x
                            hovered = proxy.value(atX: x, as: Date.self).flatMap { Self.nearest(points, to: $0) }
                        case .ended:
                            hovered = nil
                        }
                    }
            }
        }
    }

    static func nearest(_ points: [UsageRatioPoint], to date: Date) -> UsageRatioPoint? {
        points.min { abs($0.date.timeIntervalSince(date)) < abs($1.date.timeIntervalSince(date)) }
    }
}

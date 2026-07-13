// ClaudeDashboard/Views/InteractiveChartContainer.swift
import SwiftUI

// MARK: - Enums

enum InteractionMode {
    case pan
    case zoom
}

enum TimeRangePreset: String, CaseIterable, Identifiable {
    case fiveHour = "5h"
    case day = "24h"
    case threeDay = "3d"
    case week = "7d"
    case month = "30d"

    var id: String { rawValue }

    var seconds: TimeInterval {
        switch self {
        case .fiveHour: return 5 * 3600
        case .day: return 86400
        case .threeDay: return 3 * 86400
        case .week: return 7 * 86400
        case .month: return 30 * 86400
        }
    }

    /// The trailing window of this preset's duration ending at `end`. Single source
    /// for how a preset maps to a visible range (init, preset buttons, live-edge follow).
    func range(endingAt end: Date) -> ClosedRange<Date> {
        end.addingTimeInterval(-seconds)...end
    }
}

// MARK: - InteractiveChartContainer

struct InteractiveChartContainer<ChartContent: View, ToolbarExtra: View>: View {
    // MARK: External

    let chartContent: (ClosedRange<Date>) -> ChartContent
    let toolbarExtra: () -> ToolbarExtra
    let dataPoints: [UsageLogEntry]
    let averageRateProvider: ((ClosedRange<Date>, [UsageLogEntry]) -> Double?)?
    let onRangeChanged: (ClosedRange<Date>) -> Void
    let chartHeight: CGFloat
    let isInteractionDisabled: Bool
    /// Bumped by the parent whenever new data may have been recorded. When the chart
    /// is following the live edge, this drives the visible window forward.
    let liveTick: Date
    /// Whether the window should auto-advance to `now` on each `liveTick`. Callers set
    /// this to false while showing a frozen/historical view (e.g. a selected cycle).
    let autoFollowsLiveEdge: Bool

    // MARK: State

    @State private var visibleRange: ClosedRange<Date>
    @State private var mode: InteractionMode = .pan
    @State private var selectedPreset: TimeRangePreset?
    @State private var dragStartRange: ClosedRange<Date>?
    @State private var zoomSelectionRange: (start: CGFloat, end: CGFloat)?

    // MARK: Init

    init(
        initialPreset: TimeRangePreset = .day,
        dataPoints: [UsageLogEntry],
        chartHeight: CGFloat = 300,
        isInteractionDisabled: Bool = false,
        liveTick: Date,
        autoFollowsLiveEdge: Bool,
        averageRateProvider: ((ClosedRange<Date>, [UsageLogEntry]) -> Double?)? = nil,
        onRangeChanged: @escaping (ClosedRange<Date>) -> Void,
        @ViewBuilder chartContent: @escaping (ClosedRange<Date>) -> ChartContent,
        @ViewBuilder toolbarExtra: @escaping () -> ToolbarExtra
    ) {
        self.dataPoints = dataPoints
        self.chartHeight = chartHeight
        self.isInteractionDisabled = isInteractionDisabled
        self.liveTick = liveTick
        self.autoFollowsLiveEdge = autoFollowsLiveEdge
        self.averageRateProvider = averageRateProvider
        self.onRangeChanged = onRangeChanged
        self.chartContent = chartContent
        self.toolbarExtra = toolbarExtra

        _visibleRange = State(initialValue: initialPreset.range(endingAt: Date()))
        _selectedPreset = State(initialValue: initialPreset)
    }

    // MARK: Body

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            chartArea
        }
        .onChange(of: liveTick) { _ in handleLiveTick() }
    }

    /// Reacts to a data refresh: slides the window to end at `now` while following the
    /// live edge, then reloads the (possibly advanced) window so freshly recorded points
    /// appear. This is the single reload path per refresh — parents only bootstrap the
    /// empty state.
    ///
    /// - Following the live edge means a preset (5h/24h/…) is active and the caller opted
    ///   in via `autoFollowsLiveEdge`; the window is defined as "last N ending now" so it
    ///   should track new points. A manual pan/zoom clears the preset, freezing the window.
    /// - An in-progress interaction is left untouched: the measure tool
    ///   (`isInteractionDisabled`) anchors on absolute dates, and a live pan/zoom drag
    ///   (`dragStartRange`/`zoomSelectionRange` still set) would commit against a shifted
    ///   domain. Their own gesture end reloads when finished.
    private func handleLiveTick() {
        guard !isInteractionDisabled, dragStartRange == nil, zoomSelectionRange == nil else { return }

        if autoFollowsLiveEdge, let preset = selectedPreset {
            visibleRange = preset.range(endingAt: Date())
        }
        onRangeChanged(visibleRange)
    }

    // MARK: - Toolbar

    private var toolbar: some View {
        HStack(spacing: 6) {
            Group {
                // Mode toggle
                Button {
                    mode = (mode == .pan) ? .zoom : .pan
                } label: {
                    Image(systemName: mode == .pan ? "hand.raised" : "magnifyingglass")
                        .frame(width: 20, height: 20)
                }
                .buttonStyle(.borderless)
                .help(mode == .pan ? "Switch to zoom mode" : "Switch to pan mode")

                Divider().frame(height: 16)

                // Preset buttons
                ForEach(TimeRangePreset.allCases) { preset in
                    presetButton(preset)
                }

                // Custom indicator
                if selectedPreset == nil {
                    Text("Custom")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 4)
                }
            }
            .disabled(isInteractionDisabled)
            .opacity(isInteractionDisabled ? 0.4 : 1)

            Spacer()

            averageRateOverlay
                .allowsHitTesting(false)

            Spacer()

            toolbarExtra()
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
    }

    private func presetButton(_ preset: TimeRangePreset) -> some View {
        let isSelected = selectedPreset == preset
        return Button {
            applyPreset(preset)
        } label: {
            Text(preset.rawValue)
                .font(.caption.bold())
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(
                    isSelected
                        ? Color.accentColor.opacity(0.2)
                        : Color.clear,
                    in: RoundedRectangle(cornerRadius: 4)
                )
        }
        .buttonStyle(.borderless)
    }

    // MARK: - Chart Area

    private var chartArea: some View {
        GeometryReader { geometry in
            ZStack(alignment: .topTrailing) {
                // Chart content — gesture applied here so onContinuousHover in chartOverlay still works
                chartContent(visibleRange)
                    .simultaneousGesture(
                        combinedGesture(geometry: geometry),
                        including: isInteractionDisabled ? .subviews : .all
                    )

                // Zoom selection highlight (drawn on top during zoom drag)
                if mode == .zoom, let zsr = zoomSelectionRange {
                    zoomSelectionHighlight(zsr, in: geometry)
                        .allowsHitTesting(false)
                }
            }
        }
        .frame(height: chartHeight)
    }

    // MARK: - Average Rate Overlay

    private var averageRateOverlay: some View {
        let rate = computeAverageRate()
        return VStack(alignment: .trailing, spacing: 0) {
            Group {
                if let rate = rate {
                    Text(String(format: "Avg: %.1f%%/h", rate))
                        .foregroundStyle(rate > 0.1 ? Color.orange : Color.green)
                } else {
                    Text("Avg: --")
                        .foregroundStyle(.secondary)
                }
            }
            .font(.caption2.monospacedDigit())
            .padding(.horizontal, 6)
            .padding(.vertical, 4)
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 4))
        }
    }

    private func computeAverageRate() -> Double? {
        // Use custom provider if supplied
        if let provider = averageRateProvider {
            return provider(visibleRange, dataPoints)
        }

        // Default: filter to visible range
        let visible = dataPoints
            .filter { $0.recordedAt >= visibleRange.lowerBound && $0.recordedAt <= visibleRange.upperBound }
            .sorted { $0.recordedAt < $1.recordedAt }

        guard visible.count >= 2 else { return nil }

        // Sum positive deltas only (skip negative drops = cycle resets)
        var positiveSum = 0.0
        for i in 1..<visible.count {
            let delta = visible[i].utilization - visible[i - 1].utilization
            if delta > 0 {
                positiveSum += delta
            }
        }

        let totalHours = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound) / 3600
        guard totalHours > 0 else { return nil }

        return positiveSum / totalHours
    }

    // MARK: - Zoom Selection Highlight

    private func zoomSelectionHighlight(_ zsr: (start: CGFloat, end: CGFloat), in geometry: GeometryProxy) -> some View {
        let minX = min(zsr.start, zsr.end)
        let width = abs(zsr.end - zsr.start)

        return Rectangle()
            .fill(Color.accentColor.opacity(0.15))
            .overlay(
                Rectangle()
                    .stroke(Color.accentColor.opacity(0.5), lineWidth: 1)
            )
            .frame(width: max(width, 1), height: geometry.size.height)
            .position(x: minX + width / 2, y: geometry.size.height / 2)
    }

    // MARK: - Gesture

    private func combinedGesture(geometry: GeometryProxy) -> some Gesture {
        DragGesture(minimumDistance: 2)
            .onChanged { value in
                if mode == .pan {
                    if dragStartRange == nil {
                        dragStartRange = visibleRange
                    }

                    guard let startRange = dragStartRange else { return }

                    let width = geometry.size.width
                    guard width > 0 else { return }

                    let duration = startRange.upperBound.timeIntervalSince(startRange.lowerBound)
                    let secondsPerPoint = duration / Double(width)
                    let offsetSeconds = Double(value.translation.width) * secondsPerPoint

                    // Shift range left (drag right = go back in time)
                    var newLower = startRange.lowerBound.addingTimeInterval(-offsetSeconds)
                    var newUpper = startRange.upperBound.addingTimeInterval(-offsetSeconds)

                    // Clamp: don't go past now
                    let now = Date()
                    if newUpper > now {
                        let excess = newUpper.timeIntervalSince(now)
                        newLower = newLower.addingTimeInterval(-excess)
                        newUpper = now
                    }

                    visibleRange = newLower...newUpper
                    selectedPreset = nil
                    onRangeChanged(visibleRange)
                } else {
                    zoomSelectionRange = (
                        start: value.startLocation.x,
                        end: value.location.x
                    )
                }
            }
            .onEnded { value in
                if mode == .pan {
                    dragStartRange = nil
                } else {
                    defer { zoomSelectionRange = nil }

                    let startX = min(value.startLocation.x, value.location.x)
                    let endX = max(value.startLocation.x, value.location.x)
                    let width = geometry.size.width

                    guard width > 0, endX - startX > 1 else { return }

                    let duration = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound)
                    let secondsPerPoint = duration / Double(width)

                    let newLower = visibleRange.lowerBound.addingTimeInterval(Double(startX) * secondsPerPoint)
                    let newUpper = visibleRange.lowerBound.addingTimeInterval(Double(endX) * secondsPerPoint)

                    // Minimum zoom: 5 minutes
                    let minInterval: TimeInterval = 5 * 60
                    guard newUpper.timeIntervalSince(newLower) >= minInterval else { return }

                    visibleRange = newLower...newUpper
                    selectedPreset = nil
                    onRangeChanged(visibleRange)
                }
            }
    }

    // MARK: - Helpers

    private func applyPreset(_ preset: TimeRangePreset) {
        visibleRange = preset.range(endingAt: Date())
        selectedPreset = preset
        onRangeChanged(visibleRange)
    }
}

# Interactive Chart Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add interactive pan, zoom, time range presets, average rate overlay, and collapsible reset cycles to both chart views via a reusable container.

**Architecture:** A new `InteractiveChartContainer` generic view wraps chart content with a gesture layer, toolbar (mode toggle + presets), and avg rate overlay. Both `OverviewChartView` and `AccountDetailView` adopt it, providing their chart marks and data while the container handles all interaction state. Reset cycles in AccountDetailView become collapsible outside the container.

**Tech Stack:** SwiftUI, Swift Charts, DragGesture, GeometryReader

---

### Task 1: TimeRangePreset Enum & InteractionMode

**Files:**
- Create: `ClaudeDashboard/Views/InteractiveChartContainer.swift`

- [ ] **Step 1: Create InteractiveChartContainer.swift with enums and placeholder struct**

```swift
// ClaudeDashboard/Views/InteractiveChartContainer.swift
import SwiftUI
import Charts

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
}

struct InteractiveChartContainer<ChartContent: View, ToolbarExtra: View>: View {
    let chartContent: ChartContent
    let toolbarExtra: ToolbarExtra
    let dataPoints: [UsageLogEntry]
    let averageRateProvider: ((ClosedRange<Date>, [UsageLogEntry]) -> Double?)?
    let onRangeChanged: (ClosedRange<Date>) -> Void
    let chartHeight: CGFloat

    @State private var visibleRange: ClosedRange<Date>
    @State private var mode: InteractionMode = .pan
    @State private var selectedPreset: TimeRangePreset?

    init(
        initialPreset: TimeRangePreset = .day,
        dataPoints: [UsageLogEntry],
        chartHeight: CGFloat = 300,
        averageRateProvider: ((ClosedRange<Date>, [UsageLogEntry]) -> Double?)? = nil,
        onRangeChanged: @escaping (ClosedRange<Date>) -> Void,
        @ViewBuilder chartContent: () -> ChartContent,
        @ViewBuilder toolbarExtra: () -> ToolbarExtra
    ) {
        self.chartContent = chartContent()
        self.toolbarExtra = toolbarExtra()
        self.dataPoints = dataPoints
        self.averageRateProvider = averageRateProvider
        self.onRangeChanged = onRangeChanged
        self.chartHeight = chartHeight
        let now = Date()
        let start = now.addingTimeInterval(-initialPreset.seconds)
        _visibleRange = State(initialValue: start...now)
        _selectedPreset = State(initialValue: initialPreset)
    }

    var body: some View {
        Text("Placeholder")
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

If it fails with "no such file", run `xcodegen generate` first since the new file needs to be picked up from the `ClaudeDashboard/` source directory (project.yml uses directory-based sources, so no project.yml change needed).

Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/InteractiveChartContainer.swift
git commit -m "feat: add InteractiveChartContainer skeleton with enums"
```

---

### Task 2: Toolbar Row (Mode Toggle + Presets)

**Files:**
- Modify: `ClaudeDashboard/Views/InteractiveChartContainer.swift`

- [ ] **Step 1: Replace the placeholder body with toolbar + chart area layout**

Replace the `body` in `InteractiveChartContainer`:

```swift
var body: some View {
    VStack(spacing: 0) {
        // Toolbar row
        HStack(spacing: 8) {
            // Mode toggle
            Button {
                mode = (mode == .pan) ? .zoom : .pan
            } label: {
                Image(systemName: mode == .pan ? "hand.raised" : "magnifyingglass")
                    .font(.caption)
                    .frame(width: 24, height: 24)
            }
            .buttonStyle(.borderless)
            .help(mode == .pan ? "Pan mode" : "Zoom mode")

            // Preset buttons
            ForEach(TimeRangePreset.allCases) { preset in
                Button {
                    selectPreset(preset)
                } label: {
                    Text(preset.rawValue)
                        .font(.caption2.bold())
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(
                            selectedPreset == preset
                                ? Color.accentColor.opacity(0.2)
                                : Color.clear
                        )
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }
                .buttonStyle(.borderless)
            }

            // Custom indicator
            if selectedPreset == nil {
                Text("Custom")
                    .font(.caption2.bold())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }

            Spacer()

            toolbarExtra
        }
        .padding(.horizontal)
        .padding(.vertical, 6)

        // Chart area (placeholder for now)
        chartContent
            .frame(height: chartHeight)
            .padding(.horizontal)
    }
}

private func selectPreset(_ preset: TimeRangePreset) {
    let now = Date()
    let start = now.addingTimeInterval(-preset.seconds)
    visibleRange = start...now
    selectedPreset = preset
    onRangeChanged(visibleRange)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/InteractiveChartContainer.swift
git commit -m "feat: add toolbar row with mode toggle and preset buttons"
```

---

### Task 3: Pan Gesture

**Files:**
- Modify: `ClaudeDashboard/Views/InteractiveChartContainer.swift`

- [ ] **Step 1: Add pan gesture state and overlay**

Add these state properties to `InteractiveChartContainer`:

```swift
@State private var dragStartLocation: CGFloat?
@State private var dragStartRange: ClosedRange<Date>?
```

Replace the chart area section in `body` (the `chartContent.frame(height:).padding(.horizontal)` block) with:

```swift
// Chart area with gesture overlay
ZStack(alignment: .topTrailing) {
    chartContent
        .frame(height: chartHeight)

    // Avg rate overlay placeholder
    avgRateOverlay
        .padding(8)
}
.padding(.horizontal)
.overlay {
    gestureOverlay
}
```

Add computed properties and gesture overlay:

```swift
@ViewBuilder
private var avgRateOverlay: some View {
    // Will be implemented in Task 5
    EmptyView()
}

private var gestureOverlay: some View {
    GeometryReader { geo in
        Rectangle().fill(.clear).contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 5)
                    .onChanged { value in
                        if mode == .pan {
                            handlePanChanged(value: value, geoWidth: geo.size.width)
                        }
                    }
                    .onEnded { value in
                        if mode == .pan {
                            handlePanEnded()
                        }
                    }
            )
    }
}

private func handlePanChanged(value: DragGesture.Value, geoWidth: CGFloat) {
    if dragStartRange == nil {
        dragStartRange = visibleRange
    }
    guard let startRange = dragStartRange else { return }

    let duration = startRange.upperBound.timeIntervalSince(startRange.lowerBound)
    let pixelToSeconds = duration / geoWidth
    let offsetSeconds = -Double(value.translation.width) * pixelToSeconds

    let newStart = startRange.lowerBound.addingTimeInterval(offsetSeconds)
    let newEnd = startRange.upperBound.addingTimeInterval(offsetSeconds)

    // Clamp: don't go past now
    let now = Date()
    if newEnd > now {
        let clamped = now.addingTimeInterval(-duration)
        visibleRange = clamped...now
    } else {
        visibleRange = newStart...newEnd
    }

    selectedPreset = nil
    onRangeChanged(visibleRange)
}

private func handlePanEnded() {
    dragStartRange = nil
}
```

- [ ] **Step 2: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/InteractiveChartContainer.swift
git commit -m "feat: add pan gesture to InteractiveChartContainer"
```

---

### Task 4: Zoom-Select Gesture

**Files:**
- Modify: `ClaudeDashboard/Views/InteractiveChartContainer.swift`

- [ ] **Step 1: Add zoom selection state and gesture handling**

Add state property:

```swift
@State private var zoomSelectionRange: (start: CGFloat, end: CGFloat)?
```

Update the `gestureOverlay` to handle zoom mode as well. Replace the existing `gestureOverlay`:

```swift
private var gestureOverlay: some View {
    GeometryReader { geo in
        ZStack {
            Rectangle().fill(.clear).contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 5)
                        .onChanged { value in
                            if mode == .pan {
                                handlePanChanged(value: value, geoWidth: geo.size.width)
                            } else {
                                handleZoomChanged(value: value, geoWidth: geo.size.width)
                            }
                        }
                        .onEnded { value in
                            if mode == .pan {
                                handlePanEnded()
                            } else {
                                handleZoomEnded(geoWidth: geo.size.width)
                            }
                        }
                )

            // Zoom selection highlight
            if mode == .zoom, let sel = zoomSelectionRange {
                let minX = min(sel.start, sel.end)
                let maxX = max(sel.start, sel.end)
                Rectangle()
                    .fill(Color.accentColor.opacity(0.15))
                    .border(Color.accentColor.opacity(0.4), width: 1)
                    .frame(width: maxX - minX)
                    .position(x: (minX + maxX) / 2, y: geo.size.height / 2)
                    .frame(height: geo.size.height)
                    .allowsHitTesting(false)
            }
        }
    }
}

private func handleZoomChanged(value: DragGesture.Value, geoWidth: CGFloat) {
    let startX = max(0, min(value.startLocation.x, geoWidth))
    let currentX = max(0, min(value.location.x, geoWidth))
    zoomSelectionRange = (start: startX, end: currentX)
}

private func handleZoomEnded(geoWidth: CGFloat) {
    guard let sel = zoomSelectionRange else { return }
    zoomSelectionRange = nil

    let minFraction = Double(min(sel.start, sel.end)) / Double(geoWidth)
    let maxFraction = Double(max(sel.start, sel.end)) / Double(geoWidth)

    let duration = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound)
    let newStart = visibleRange.lowerBound.addingTimeInterval(duration * minFraction)
    let newEnd = visibleRange.lowerBound.addingTimeInterval(duration * maxFraction)

    // Minimum zoom: 5 minutes
    let minDuration: TimeInterval = 5 * 60
    guard newEnd.timeIntervalSince(newStart) >= minDuration else { return }

    visibleRange = newStart...newEnd
    selectedPreset = nil
    onRangeChanged(visibleRange)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/InteractiveChartContainer.swift
git commit -m "feat: add zoom-select gesture to InteractiveChartContainer"
```

---

### Task 5: Average Rate Overlay

**Files:**
- Modify: `ClaudeDashboard/Views/InteractiveChartContainer.swift`

- [ ] **Step 1: Implement the avg rate calculation and overlay view**

Replace the `avgRateOverlay` computed property:

```swift
@ViewBuilder
private var avgRateOverlay: some View {
    let rate = computeAverageRate()
    VStack(spacing: 0) {
        if let rate {
            Text(String(format: "Avg: %.1f%%/h", rate))
                .font(.caption2.bold().monospacedDigit())
                .foregroundStyle(rate > 0.1 ? .orange : .green)
        } else {
            Text("Avg: --")
                .font(.caption2.bold().monospacedDigit())
                .foregroundStyle(.secondary)
        }
    }
    .padding(.horizontal, 6)
    .padding(.vertical, 3)
    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 4))
}

private func computeAverageRate() -> Double? {
    // Use custom provider if supplied (e.g., OverviewChartView passes total line computation)
    if let provider = averageRateProvider {
        return provider(visibleRange, dataPoints)
    }

    // Default: compute from raw data points (works for single-account views)
    let visible = dataPoints.filter {
        $0.recordedAt >= visibleRange.lowerBound && $0.recordedAt <= visibleRange.upperBound
    }.sorted { $0.recordedAt < $1.recordedAt }

    guard visible.count >= 2 else { return nil }

    let totalHours = visibleRange.upperBound.timeIntervalSince(visibleRange.lowerBound) / 3600
    guard totalHours > 0.01 else { return nil }

    var positiveDeltas = 0.0
    for i in 1..<visible.count {
        let delta = visible[i].utilization - visible[i - 1].utilization
        // Skip reset drops (>10% decrease = cycle reset)
        if delta >= 0 {
            positiveDeltas += delta
        }
    }

    return positiveDeltas / totalHours
}
```

- [ ] **Step 2: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 3: Commit**

```bash
git add ClaudeDashboard/Views/InteractiveChartContainer.swift
git commit -m "feat: add average rate overlay to InteractiveChartContainer"
```

---

### Task 6: Integrate InteractiveChartContainer into OverviewChartView

**Files:**
- Modify: `ClaudeDashboard/Views/OverviewChartView.swift`

- [ ] **Step 1: Remove old TimeRange enum and time range picker**

Delete the `TimeRange` enum (lines 20-34) from `OverviewChartView`.

Replace the `timeRange` state property:

```swift
// Remove this:
@State private var timeRange: TimeRange = .day

// Add this:
@State private var visibleRange: ClosedRange<Date> = {
    let now = Date()
    return now.addingTimeInterval(-86400)...now
}()
```

- [ ] **Step 2: Remove the old controls HStack and wrap chart in InteractiveChartContainer**

Replace the Controls section and Chart section in `body` (from the `// Controls` comment through `overviewChart.padding()`) with:

```swift
// Interactive chart
InteractiveChartContainer(
    initialPreset: .day,
    dataPoints: logs,
    chartHeight: 300,
    averageRateProvider: { range, allLogs in
        // Compute avg rate from the weighted total line, not individual account logs
        let totalPoints = computeTotalLine().filter {
            $0.time >= range.lowerBound && $0.time <= range.upperBound
        }
        guard totalPoints.count >= 2 else { return nil }
        let totalHours = range.upperBound.timeIntervalSince(range.lowerBound) / 3600
        guard totalHours > 0.01 else { return nil }
        var positiveDeltas = 0.0
        for i in 1..<totalPoints.count {
            let delta = totalPoints[i].value - totalPoints[i - 1].value
            if delta >= 0 { positiveDeltas += delta }
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
        .frame(width: 140)
    }
)
```

- [ ] **Step 3: Update overviewChart to remove the wrapping ZStack and frame**

The `overviewChart` property currently returns a `ZStack` with `.frame(height: 300)`. Remove the `ZStack` wrapper and `.frame(height: 300)` since the container handles height. Keep only the `Chart { ... }` with its modifiers (chartForegroundStyleScale, chartYScale, chartYAxis, chartLegend, chartOverlay) and the hover tooltip overlay.

The `overviewChart` should be:

```swift
private var overviewChart: some View {
    ZStack(alignment: .topTrailing) {
        Chart {
            // ... all existing chart content (per-account lines, total line, limit markers, hover line) unchanged ...
        }
        .chartForegroundStyleScale(domain: chartColorDomain, range: chartColorRange)
        .chartYScale(domain: 0...105)
        .chartYAxis { /* ... unchanged ... */ }
        .chartLegend(.hidden)
        .chartOverlay { proxy in
            GeometryReader { _ in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onContinuousHover { phase in
                        switch phase {
                        case .active(let location):
                            hoverDate = proxy.value(atX: location.x)
                        case .ended:
                            hoverDate = nil
                        }
                    }
            }
        }

        // Hover tooltip
        if let hoverDate {
            hoverTooltip(for: hoverDate)
                .padding(8)
        }
    }
}
```

- [ ] **Step 4: Update loadLogs to accept a range parameter**

Replace the `loadLogs()` function:

```swift
private func loadLogs(range: ClosedRange<Date>? = nil) async {
    if selectedAccounts.isEmpty {
        selectedAccounts = Set(viewModel.accountStates.map(\.id))
    }

    let effectiveRange = range ?? visibleRange
    let store = viewModel.logStore
    logs = await store.allLogs(window: selectedWindow, from: effectiveRange.lowerBound, to: effectiveRange.upperBound)
}
```

Update the `.onChange(of: timeRange)` to `.onChange(of: selectedWindow)`. Remove the duplicate `.onChange(of: selectedWindow)` if there are two. The final `onChange` and `task` should be:

```swift
.task { await loadLogs() }
.onChange(of: selectedWindow) { _ in Task { await loadLogs() } }
```

Remove the `.onChange(of: timeRange)` line entirely.

- [ ] **Step 5: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 6: Commit**

```bash
git add ClaudeDashboard/Views/OverviewChartView.swift
git commit -m "feat: integrate InteractiveChartContainer into OverviewChartView"
```

---

### Task 7: Integrate InteractiveChartContainer into AccountDetailView

**Files:**
- Modify: `ClaudeDashboard/Views/AccountDetailView.swift`
- Modify: `ClaudeDashboard/ViewModels/AccountDetailViewModel.swift`

- [ ] **Step 1: Add visibleRange support to AccountDetailViewModel**

Add a `visibleRange` property and update `loadData` to use it. Add to `AccountDetailViewModel`:

```swift
@Published var visibleRange: ClosedRange<Date> = {
    let now = Date()
    return now.addingTimeInterval(-86400)...now
}()

func updateRange(_ range: ClosedRange<Date>) {
    visibleRange = range
    Task { await loadData() }
}
```

Update `loadData()` — when no cycle is selected, use `visibleRange` for the query bounds:

```swift
func loadData() async {
    let cycles = await logStore.resetCycles(accountId: accountId, window: selectedWindow)
    resetCycles = cycles

    if let cycle = selectedCycle {
        let cycleLogs = await logStore.logs(
            accountId: accountId, window: selectedWindow,
            from: cycle.firstRecordedAt.addingTimeInterval(-1),
            to: cycle.resetsAt
        )
        logs = cycleLogs
    } else {
        let allLogs = await logStore.logs(
            accountId: accountId, window: selectedWindow,
            from: visibleRange.lowerBound, to: visibleRange.upperBound
        )
        logs = allLogs
    }
}
```

- [ ] **Step 2: Wrap chart in InteractiveChartContainer in AccountDetailView**

Replace the window picker HStack and chart section (from `// Window picker` comment through `usageChart.padding()`) in `AccountDetailView.body` with:

```swift
// Interactive chart
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
```

- [ ] **Step 3: Update usageChart to remove the ZStack wrapper and frame**

Similar to OverviewChartView — the `usageChart` property should return the `ZStack` with the `Chart` and hover tooltip, but without `.frame(height: 250)` since the container sets height:

```swift
private var usageChart: some View {
    ZStack(alignment: .topTrailing) {
        Chart {
            // ... all existing chart content unchanged ...
        }
        .chartYScale(domain: 0...105)
        .chartYAxis { /* ... unchanged ... */ }
        .chartOverlay { proxy in
            GeometryReader { _ in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onContinuousHover { phase in
                        switch phase {
                        case .active(let location):
                            hoverDate = proxy.value(atX: location.x)
                        case .ended:
                            hoverDate = nil
                        }
                    }
            }
        }

        // Hover tooltip
        if let hoverDate {
            hoverTooltip(for: hoverDate)
                .padding(8)
        }
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Views/AccountDetailView.swift ClaudeDashboard/ViewModels/AccountDetailViewModel.swift
git commit -m "feat: integrate InteractiveChartContainer into AccountDetailView"
```

---

### Task 8: Collapsible Reset Cycles

**Files:**
- Modify: `ClaudeDashboard/Views/AccountDetailView.swift`

- [ ] **Step 1: Add cyclesExpanded state**

Add to the `@State` properties in `AccountDetailView`:

```swift
@State private var cyclesExpanded = false
```

- [ ] **Step 2: Replace resetCyclesList with collapsible version**

Replace the entire `resetCyclesList` computed property:

```swift
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
```

- [ ] **Step 3: Verify it compiles**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5`

Expected: BUILD SUCCEEDED

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/AccountDetailView.swift
git commit -m "feat: make reset cycles collapsible (default collapsed)"
```

---

### Task 9: Build & Manual Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full build**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -20`

Expected: BUILD SUCCEEDED

- [ ] **Step 2: Run existing tests**

Run: `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -20`

Expected: All existing tests pass (test suite doesn't test views directly, so no new test failures expected).

- [ ] **Step 3: Verify InteractiveChartContainer.swift is included in build**

Run: `grep -r "InteractiveChartContainer" ClaudeDashboard/Views/OverviewChartView.swift ClaudeDashboard/Views/AccountDetailView.swift`

Expected: Both files reference `InteractiveChartContainer`.

- [ ] **Step 4: Verify old TimeRange enum is removed**

Run: `grep -n "enum TimeRange" ClaudeDashboard/Views/OverviewChartView.swift`

Expected: No output (enum removed).

- [ ] **Step 5: Commit any fixes if needed, then final commit**

If all checks pass, no commit needed. If fixes were required, commit them:

```bash
git add -A
git commit -m "fix: resolve build issues from interactive chart integration"
```

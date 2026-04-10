# Reset Countdown Bars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add visual countdown bar segments below each usage progress bar showing time remaining until reset.

**Architecture:** Single-file change to `UsageBar.swift`. Extract a `CountdownBarsView` sub-view for the segmented countdown row. The existing `resetsAt` and `totalSeconds` properties already provide all needed data.

**Tech Stack:** SwiftUI, GeometryReader

---

### Task 1: Add CountdownBarsView sub-view

**Files:**
- Modify: `ClaudeDashboard/Views/UsageBar.swift`

- [ ] **Step 1: Add the `segmentCount` computed property to `UsageBar`**

Add this computed property inside the `UsageBar` struct, after the `init`:

```swift
/// Number of countdown segments: 5 for 5h window, 7 for 7d window.
private var segmentCount: Int {
    totalSeconds <= 18000 ? 5 : 7
}
```

- [ ] **Step 2: Add the `CountdownBarsView` sub-view**

Add this private struct at the bottom of `UsageBar.swift`, outside the `UsageBar` struct:

```swift
private struct CountdownBarsView: View {
    let resetsAt: Date
    let totalSeconds: TimeInterval
    let segmentCount: Int

    private let segmentColor = Color(red: 74/255, green: 144/255, blue: 217/255) // #4a90d9
    private let depletedColor = Color.primary.opacity(0.08)

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: 2) {
                ForEach(0..<segmentCount, id: \.self) { index in
                    segmentView(index: index)
                }
            }
            .frame(width: geo.size.width * 2.0 / 3.0, height: 8)
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .frame(height: 8)
    }

    private func segmentView(index: Int) -> some View {
        let fillFraction = fillFraction(for: index)
        return GeometryReader { geo in
            ZStack(alignment: .leading) {
                // Full background (depleted color)
                RoundedRectangle(cornerRadius: 2)
                    .fill(depletedColor)

                // Blue fill from the right
                if fillFraction > 0 {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(segmentColor)
                        .frame(width: geo.size.width * fillFraction)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
        }
    }

    /// Returns 0.0 (fully depleted) to 1.0 (fully remaining) for a segment.
    private func fillFraction(for index: Int) -> Double {
        let remaining = resetsAt.timeIntervalSinceNow
        guard remaining > 0, totalSeconds > 0 else { return 0 }

        let elapsed = max(0, totalSeconds - remaining)
        let secondsPerSegment = totalSeconds / Double(segmentCount)
        let segmentStart = Double(index) * secondsPerSegment
        let segmentEnd = Double(index + 1) * secondsPerSegment

        if elapsed >= segmentEnd {
            return 0.0 // fully depleted
        } else if elapsed <= segmentStart {
            return 1.0 // fully remaining
        } else {
            return (segmentEnd - elapsed) / secondsPerSegment
        }
    }
}
```

- [ ] **Step 3: Build to verify no compile errors**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 4: Commit**

```bash
git add ClaudeDashboard/Views/UsageBar.swift
git commit -m "feat: add CountdownBarsView sub-view with segment depletion logic"
```

---

### Task 2: Integrate countdown row into UsageBar layout

**Files:**
- Modify: `ClaudeDashboard/Views/UsageBar.swift`

- [ ] **Step 1: Widen the percentage text to 50px**

In `UsageBar.body`, change the percentage `Text` frame width from `36` to `50`:

```swift
// old
.frame(width: 36, alignment: .trailing)

// new
.frame(width: 50, alignment: .trailing)
```

- [ ] **Step 2: Replace the "resets in" text with the countdown row**

Replace the existing reset text block:

```swift
if let resetsAt {
    Text("resets in \(formatTimeRemaining(resetsAt))")
        .font(.caption2)
        .foregroundStyle(resetUrgencyColor(resetsAt))
        .padding(.leading, 28)
}
```

With the countdown row:

```swift
if let resetsAt {
    HStack(spacing: 8) {
        // Spacer matching label width
        Color.clear
            .frame(width: 24)

        CountdownBarsView(
            resetsAt: resetsAt,
            totalSeconds: totalSeconds,
            segmentCount: segmentCount
        )

        Text(formatTimeRemaining(resetsAt))
            .font(.system(.caption, design: .monospaced))
            .foregroundStyle(resetUrgencyColor(resetsAt))
            .frame(width: 50, alignment: .trailing)
    }
}
```

- [ ] **Step 3: Build to verify**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 4: Run existing tests to verify no regressions**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test 2>&1 | tail -10
```
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add ClaudeDashboard/Views/UsageBar.swift
git commit -m "feat: integrate countdown bars row into UsageBar layout"
```

---

### Task 3: Visual verification

- [ ] **Step 1: Run the app and verify visually**

Run:
```bash
xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -5
```
Then launch the built app:
```bash
open "$(xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard -showBuildSettings 2>/dev/null | grep ' BUILT_PRODUCTS_DIR' | awk '{print $3}')/ClaudeDashboard.app"
```

Verify:
- Each usage bar (5h, 7d, S) has a countdown bar row below the progress bar
- 5h shows 5 segments, 7d/S shows 7 segments
- Countdown bars are right-aligned, matching the right edge of the progress bar
- Text shows compact format ("2d 8h", "3h 20m") at same font size as percentage
- Segments deplete from left to right correctly

- [ ] **Step 2: Final commit if any adjustments were needed**

```bash
git add ClaudeDashboard/Views/UsageBar.swift
git commit -m "fix: adjust countdown bar visual tweaks"
```

Only commit if changes were made in this step.

// The arithmetic behind the usage charts, kept apart from Cairo so it can be
// tested. Ports what SwiftUI's Charts does implicitly for
// AccountDetailView.swift and OverviewChartView.swift: map a (time,
// utilization) series into plot coordinates, choose axis ticks, and answer
// "which sample is the pointer over".
//
// The y axis is always 0-100: utilization is a percentage, and macOS pins the
// domain with .chartYScale(domain: 0...100) so two charts are comparable at a
// glance rather than each autoscaling to its own data.
export const Y_MIN = 0;
export const Y_MAX = 100;

export function makeScale({fromMs, toMs, width, height, padding}) {
    const left = padding.left;
    const top = padding.top;
    const plotWidth = Math.max(1, width - padding.left - padding.right);
    const plotHeight = Math.max(1, height - padding.top - padding.bottom);
    // A zero-width time domain would divide by zero; one point is drawn at the
    // left edge instead of vanishing.
    const span = Math.max(1, toMs - fromMs);

    return {
        left, top, plotWidth, plotHeight, fromMs, toMs,
        right: left + plotWidth,
        bottom: top + plotHeight,
        x(ms) {
            return left + ((ms - fromMs) / span) * plotWidth;
        },
        y(utilization) {
            const clamped = Math.min(Y_MAX, Math.max(Y_MIN, utilization));
            return top + (1 - (clamped - Y_MIN) / (Y_MAX - Y_MIN)) * plotHeight;
        },
        msAt(px) {
            const fraction = (px - left) / plotWidth;
            return fromMs + fraction * span;
        },
        utilizationAt(py) {
            const fraction = 1 - (py - top) / plotHeight;
            return Y_MIN + fraction * (Y_MAX - Y_MIN);
        },
    };
}

// Five horizontal gridlines at 0/25/50/75/100, the same marks macOS's
// .chartYAxis default produces for a 0-100 domain.
export const Y_TICKS = [0, 25, 50, 75, 100];

// Time ticks are chosen from a fixed ladder so the labels stay readable at any
// zoom: the smallest step that yields at most `maxTicks` marks wins.
const TIME_STEPS_MS = [
    60_000,           // 1m
    5 * 60_000,       // 5m
    15 * 60_000,      // 15m
    30 * 60_000,      // 30m
    3600_000,         // 1h
    3 * 3600_000,     // 3h
    6 * 3600_000,     // 6h
    12 * 3600_000,    // 12h
    86_400_000,       // 1d
    2 * 86_400_000,   // 2d
    7 * 86_400_000,   // 7d
];

export function timeTicks(fromMs, toMs, maxTicks = 6) {
    const span = Math.max(1, toMs - fromMs);
    const step = TIME_STEPS_MS.find(s => span / s <= maxTicks) ??
        TIME_STEPS_MS[TIME_STEPS_MS.length - 1];
    // Ticks land on multiples of the step in UTC, then render in local time;
    // a step of an hour or more therefore lands on the hour for any whole-hour
    // timezone, which is every timezone the labels below distinguish.
    const first = Math.ceil(fromMs / step) * step;
    const ticks = [];
    for (let t = first; t <= toMs; t += step)
        ticks.push(t);
    return ticks;
}

// Below a day of span the label is a clock time; at or above it, a weekday and
// hour — the same split AccountDetailView's axis uses.
export function formatTick(ms, spanMs, date = new Date(ms)) {
    const hh = String(date.getHours()).padStart(2, '0');
    const mm = String(date.getMinutes()).padStart(2, '0');
    if (spanMs < 86_400_000)
        return `${hh}:${mm}`;
    const day = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][date.getDay()];
    return `${day} ${hh}:${mm}`;
}

// The nearest sample to a pointer position, or null when the series is empty.
// macOS resolves hover the same way — nearest in x, with no distance cutoff —
// so the crosshair never blanks out between two widely spaced samples.
export function nearestEntry(entries, ms) {
    if (entries.length === 0)
        return null;
    let best = entries[0];
    let bestDistance = Math.abs(best.recordedAtMs - ms);
    for (const entry of entries) {
        const distance = Math.abs(entry.recordedAtMs - ms);
        if (distance < bestDistance) {
            best = entry;
            bestDistance = distance;
        }
    }
    return best;
}

// Splits a series wherever a reset drops it to zero, so the line is drawn as
// separate strokes rather than one polyline that slopes back up through the
// gap. A segment boundary is a sample at exactly 0 following a non-zero one —
// the same rule resetCycles uses to find a cycle.
export function segments(entries) {
    const out = [];
    let current = [];
    for (let i = 0; i < entries.length; i++) {
        const entry = entries[i];
        const prev = entries[i - 1];
        if (prev && prev.utilization > 0 && entry.utilization === 0) {
            current.push(entry);
            out.push(current);
            current = [];
            continue;
        }
        current.push(entry);
    }
    if (current.length > 0)
        out.push(current);
    return out.filter(seg => seg.length > 0);
}

// Pans and zooms the visible range. `factor` below 1 zooms in; the anchor is
// the pointer position so the sample under the cursor stays put, which is what
// InteractiveChartContainer does.
export function zoomRange({fromMs, toMs}, factor, anchorMs, {minSpanMs = 60_000, maxSpanMs = 90 * 86_400_000} = {}) {
    const span = toMs - fromMs;
    const nextSpan = Math.min(maxSpanMs, Math.max(minSpanMs, span * factor));
    const anchorFraction = span === 0 ? 0.5 : (anchorMs - fromMs) / span;
    const nextFrom = anchorMs - anchorFraction * nextSpan;
    return {fromMs: nextFrom, toMs: nextFrom + nextSpan};
}

export function panRange({fromMs, toMs}, deltaMs) {
    return {fromMs: fromMs + deltaMs, toMs: toMs + deltaMs};
}

// "Show All": the full extent of the data, with a 2% margin either side so the
// first and last samples are not painted on the frame.
export function fullRange(entries, nowMs = Date.now()) {
    if (entries.length === 0)
        return {fromMs: nowMs - 86_400_000, toMs: nowMs};
    let min = entries[0].recordedAtMs;
    let max = min;
    for (const entry of entries) {
        if (entry.recordedAtMs < min) min = entry.recordedAtMs;
        if (entry.recordedAtMs > max) max = entry.recordedAtMs;
    }
    const span = Math.max(60_000, max - min);
    const margin = span * 0.02;
    return {fromMs: min - margin, toMs: max + margin};
}

// The measure tool: the delta between two picked points, and the rate that
// delta implies. A zero or negative time gap yields a null rate rather than an
// infinity, so the readout degrades to "no rate" instead of printing ∞.
export function measure(pointA, pointB) {
    const deltaUtilization = pointB.utilization - pointA.utilization;
    const deltaMs = pointB.recordedAtMs - pointA.recordedAtMs;
    const perHour = deltaMs > 0 ? deltaUtilization / (deltaMs / 3600_000) : null;
    return {deltaUtilization, deltaMs, perHour};
}

// OverviewChartView's optional total line: the summed utilization across
// accounts at each distinct timestamp, carrying each account's last known
// value forward so a series that did not report at that instant still counts.
export function totalSeries(entriesByAccount) {
    const timestamps = new Set();
    for (const entries of entriesByAccount.values()) {
        for (const entry of entries)
            timestamps.add(entry.recordedAtMs);
    }
    const sorted = [...timestamps].sort((a, b) => a - b);

    const cursors = new Map();
    const last = new Map();
    for (const key of entriesByAccount.keys()) {
        cursors.set(key, 0);
        last.set(key, null);
    }

    return sorted.map(ms => {
        let total = 0;
        for (const [key, entries] of entriesByAccount) {
            let i = cursors.get(key);
            while (i < entries.length && entries[i].recordedAtMs <= ms) {
                last.set(key, entries[i]);
                i += 1;
            }
            cursors.set(key, i);
            const seen = last.get(key);
            if (seen)
                total += seen.utilization;
        }
        return {recordedAtMs: ms, utilization: total};
    });
}

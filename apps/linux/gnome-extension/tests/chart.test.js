import {test, assertEqual, assertClose} from './harness.js';
import {
    makeScale, Y_TICKS, timeTicks, formatTick, nearestEntry, segments,
    zoomRange, panRange, fullRange, measure, totalSeries,
} from '../lib/chart.js';

const PAD = {left: 40, right: 12, top: 12, bottom: 24};
const T0 = 1_700_000_000_000;

function entry(atMs, utilization, accountId = 'a') {
    return {id: atMs, accountId, window: 0, resetsAtMs: atMs + 3600_000, recordedAtMs: atMs, utilization, isLimited: false};
}

test('the scale maps the time domain across the plot width', () => {
    const s = makeScale({fromMs: 0, toMs: 1000, width: 200, height: 100, padding: PAD});
    assertEqual(s.x(0), 40);
    assertEqual(s.x(1000), 40 + 148);
    assertClose(s.x(500), 40 + 74, 0.001);
});

test('the y domain is pinned to 0-100 and inverted for screen coordinates', () => {
    const s = makeScale({fromMs: 0, toMs: 1000, width: 200, height: 112, padding: PAD});
    assertEqual(s.y(100), 12);
    assertEqual(s.y(0), 12 + 76);
    // Out-of-domain values clamp rather than drawing outside the frame.
    assertEqual(s.y(140), s.y(100));
    assertEqual(s.y(-20), s.y(0));
});

test('an empty time domain still produces a finite x', () => {
    const s = makeScale({fromMs: 500, toMs: 500, width: 200, height: 100, padding: PAD});
    assertEqual(Number.isFinite(s.x(500)), true);
});

test('msAt and utilizationAt invert the scale', () => {
    const s = makeScale({fromMs: 0, toMs: 1000, width: 200, height: 112, padding: PAD});
    assertClose(s.msAt(s.x(250)), 250, 0.001);
    assertClose(s.utilizationAt(s.y(60)), 60, 0.001);
});

test('the y gridlines are the quartiles of a 0-100 domain', () => {
    assertEqual(Y_TICKS.join(','), '0,25,50,75,100');
});

test('time ticks pick the smallest step that fits the tick budget', () => {
    // One hour of span with a budget of 6 → 15-minute steps (4 marks), not
    // 5-minute (12 marks).
    const ticks = timeTicks(0, 3600_000, 6);
    assertEqual(ticks.length <= 6, true);
    assertEqual(ticks[1] - ticks[0], 15 * 60_000);
});

test('a very wide range falls back to the largest step rather than flooding', () => {
    const ticks = timeTicks(0, 365 * 86_400_000, 6);
    assertEqual(ticks[1] - ticks[0], 7 * 86_400_000);
});

test('ticks land inside the range', () => {
    const ticks = timeTicks(T0, T0 + 6 * 3600_000, 6);
    assertEqual(ticks.every(t => t >= T0 && t <= T0 + 6 * 3600_000), true);
});

test('tick labels switch from clock time to weekday past a day of span', () => {
    const d = new Date(2026, 0, 5, 14, 30); // a Monday
    assertEqual(formatTick(d.getTime(), 3600_000, d), '14:30');
    assertEqual(formatTick(d.getTime(), 2 * 86_400_000, d), 'Mon 14:30');
});

test('hover resolves to the nearest sample in time', () => {
    const entries = [entry(T0, 10), entry(T0 + 100_000, 20), entry(T0 + 500_000, 30)];
    assertEqual(nearestEntry(entries, T0 + 90_000).utilization, 20);
    // No distance cutoff: a pointer far past the last sample still resolves.
    assertEqual(nearestEntry(entries, T0 + 9_000_000).utilization, 30);
    assertEqual(nearestEntry([], T0), null);
});

test('the line splits at a reset drop so it is not drawn sloping back up', () => {
    const entries = [entry(T0, 40), entry(T0 + 60_000, 80), entry(T0 + 120_000, 0), entry(T0 + 180_000, 5)];
    const segs = segments(entries);
    assertEqual(segs.length, 2);
    // The zero sample closes the first segment rather than opening the second,
    // so the line falls to the axis instead of jumping.
    assertEqual(segs[0].length, 3);
    assertEqual(segs[0][2].utilization, 0);
    assertEqual(segs[1].length, 1);
});

test('a series that never resets is one segment', () => {
    assertEqual(segments([entry(T0, 1), entry(T0 + 1000, 2)]).length, 1);
    assertEqual(segments([]).length, 0);
});

test('a leading zero does not split, since nothing dropped to it', () => {
    const segs = segments([entry(T0, 0), entry(T0 + 1000, 5)]);
    assertEqual(segs.length, 1);
});

test('zoom keeps the anchored instant under the pointer', () => {
    const range = {fromMs: 0, toMs: 1000_000};
    const zoomed = zoomRange(range, 0.5, 250_000);
    assertClose((250_000 - zoomed.fromMs) / (zoomed.toMs - zoomed.fromMs), 0.25, 0.0001);
    assertClose(zoomed.toMs - zoomed.fromMs, 500_000, 0.0001);
});

test('zoom clamps to the min and max span', () => {
    const tiny = zoomRange({fromMs: 0, toMs: 60_000}, 0.01, 30_000);
    assertEqual(tiny.toMs - tiny.fromMs, 60_000);
    const huge = zoomRange({fromMs: 0, toMs: 90 * 86_400_000}, 100, 0);
    assertEqual(huge.toMs - huge.fromMs, 90 * 86_400_000);
});

test('pan shifts both bounds by the same delta', () => {
    const panned = panRange({fromMs: 0, toMs: 1000}, 250);
    assertEqual(panned.fromMs, 250);
    assertEqual(panned.toMs, 1250);
});

test('show-all spans the data with a small margin', () => {
    const r = fullRange([entry(T0, 1), entry(T0 + 1000_000, 2)]);
    assertEqual(r.fromMs < T0, true);
    assertEqual(r.toMs > T0 + 1000_000, true);
});

test('show-all on an empty series falls back to the last day', () => {
    const r = fullRange([], T0);
    assertEqual(r.toMs, T0);
    assertEqual(r.toMs - r.fromMs, 86_400_000);
});

test('the measure tool reports the delta and the implied hourly rate', () => {
    const m = measure(entry(T0, 10), entry(T0 + 3600_000, 30));
    assertEqual(m.deltaUtilization, 20);
    assertEqual(m.deltaMs, 3600_000);
    assertClose(m.perHour, 20, 0.0001);
});

test('a non-advancing measure yields no rate rather than an infinity', () => {
    assertEqual(measure(entry(T0, 10), entry(T0, 30)).perHour, null);
    assertEqual(measure(entry(T0 + 1000, 10), entry(T0, 30)).perHour, null);
});

test('the total line carries each account’s last value forward', () => {
    const byAccount = new Map([
        ['a', [entry(T0, 10, 'a'), entry(T0 + 200_000, 30, 'a')]],
        ['b', [entry(T0 + 100_000, 5, 'b')]],
    ]);
    const total = totalSeries(byAccount);
    assertEqual(total.length, 3);
    // At T0 only a has reported.
    assertEqual(total[0].utilization, 10);
    // At +100s, a's 10 is carried forward and b's 5 is added.
    assertEqual(total[1].utilization, 15);
    // At +200s, a moves to 30 and b's 5 is carried forward.
    assertEqual(total[2].utilization, 35);
});

test('the total of no accounts is an empty series', () => {
    assertEqual(totalSeries(new Map()).length, 0);
});

import {test, assertEqual, assertClose, assertDeepEqual} from './harness.js';
import {
    METRICS, START_ANGLE, TAU, fillFraction, progressArc, segmentCountFor,
    gapFraction, segmentFraction, segmentRange, countdownSegments, toAngle,
    percentFontSize, percentSignFontSize, countdownFontSize,
    ringRadius, ringCenter,
} from '../lib/geometry.js';

test('the compact metrics match the macOS popover', () => {
    assertEqual(METRICS.largeDiameter, 52);
    assertEqual(METRICS.smallDiameter, 34);
    assertEqual(METRICS.largeLineWidth, 6);
    assertEqual(METRICS.smallLineWidth, 4);
});

test('the fill fraction clamps at both ends', () => {
    assertEqual(fillFraction(-5), 0);
    assertEqual(fillFraction(50), 0.5);
    assertEqual(fillFraction(140), 1);
});

test('the progress arc starts at twelve o_clock', () => {
    assertClose(progressArc(25).start, START_ANGLE, 1e-12);
    assertClose(progressArc(25).end, START_ANGLE + 0.25 * TAU, 1e-12);
});

test('a zero-utilisation arc is marked empty so no arc is stroked', () => {
    assertEqual(progressArc(0).empty, true);
    assertEqual(progressArc(1).empty, false);
});

test('the five-hour window has five segments and the seven-day window has seven', () => {
    assertEqual(segmentCountFor(18000), 5);
    assertEqual(segmentCountFor(18001), 7);
    assertEqual(segmentCountFor(604800), 7);
});

test('segments and gaps tile the whole circle', () => {
    const d = METRICS.smallDiameter, n = 5;
    const total = n * (segmentFraction(d, n) + gapFraction(d));
    assertClose(total, 1, 1e-12);
});

test('the last segment ends one gap short of a full turn', () => {
    const d = METRICS.smallDiameter, n = 7;
    assertClose(segmentRange(n - 1, d, n).end, 1 - gapFraction(d), 1e-12);
});

test('a full countdown yields every segment', () => {
    const segs = countdownSegments(18000, 18000, METRICS.smallDiameter, 5);
    assertEqual(segs.length, 5);
    assertClose(segs[0].start, 0, 1e-12);
});

test('an elapsed countdown yields nothing to draw', () => {
    assertDeepEqual(countdownSegments(0, 18000, METRICS.smallDiameter, 5), []);
    assertDeepEqual(countdownSegments(-1, 18000, METRICS.smallDiameter, 5), []);
});

test('a partly elapsed countdown drops leading segments and clips the boundary one', () => {
    const d = METRICS.smallDiameter, n = 5;
    const segs = countdownSegments(0.5 * 18000, 18000, d, n);
    const fillStart = 0.5 * (1 - gapFraction(d));

    // Segments 2, 3 and 4 survive; segment 2 straddles the boundary and must
    // be clipped to it, not dropped. Dropping it would leave 2 segments
    // starting at 0.6 — which is the regression this pins.
    assertEqual(segs.length, 3, 'segment count');
    assertClose(segs[0].start, fillStart, 1e-12, 'boundary segment clipped to fillStart');
    assertClose(segs[0].end, segmentRange(2, d, n).end, 1e-12, 'boundary segment keeps its own end');
});

test('derived font sizes follow the diameter', () => {
    assertClose(percentFontSize(52), 17.16, 1e-9);
    assertClose(percentSignFontSize(52), 10.4, 1e-9);
    assertClose(countdownFontSize(34), 8.16, 1e-9);
});

test('a fraction maps to an angle measured from twelve o_clock', () => {
    assertClose(toAngle(0), START_ANGLE, 1e-12);
    assertClose(toAngle(0.5), START_ANGLE + Math.PI, 1e-12);
});

test('the ring radius insets by half the line width so the stroke stays in its box', () => {
    // A Cairo stroke straddles the path, so the band's outer edge lands
    // exactly on the box edge only when the radius is inset by half the width.
    const d = METRICS.largeDiameter, lw = METRICS.largeLineWidth;
    assertClose(ringRadius(d, lw) + lw / 2, d / 2, 1e-12, 'outer edge');
    assertEqual(ringRadius(52, 6), 23);
    assertEqual(ringRadius(34, 4), 15);
});

test('the ring centre is the middle of its box', () => {
    assertEqual(ringCenter(52), 26);
    assertEqual(ringCenter(34), 17);
});

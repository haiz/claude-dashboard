// Every number that decides what a ring looks like lives here, so that
// ring.js can be a Cairo layer that computes nothing. Ports the geometry of
// CircularProgressView and CircularCountdownView
// (apps/macos/ClaudeDashboard/Views/UsageBar.swift:183 and :247).

export const TAU = Math.PI * 2;

// SwiftUI strokes a trimmed Circle and then applies rotationEffect(-90°);
// Cairo measures from 3 o'clock, so the same start is -pi/2.
export const START_ANGLE = -Math.PI / 2;

export const METRICS = {
    largeDiameter: 52,
    smallDiameter: 34,
    largeLineWidth: 6,
    smallLineWidth: 4,
    windowLabelFontSize: 10,
    resetLabelFontSize: 8,
    animalFontSize: 10,
    ringGap: 5,
};

export function fillFraction(utilization) {
    return Math.min(Math.max(utilization, 0) / 100, 1);
}

export function progressArc(utilization) {
    const fraction = fillFraction(utilization);
    return {
        start: START_ANGLE,
        end: START_ANGLE + fraction * TAU,
        empty: fraction <= 0,
    };
}

export function segmentCountFor(totalSeconds) {
    return totalSeconds <= 18000 ? 5 : 7;
}

// A two-pixel gap expressed as a fraction of the circumference.
export function gapFraction(diameter) {
    return 2 / (Math.PI * diameter);
}

export function segmentFraction(diameter, segmentCount) {
    return (1 - segmentCount * gapFraction(diameter)) / segmentCount;
}

export function segmentRange(index, diameter, segmentCount) {
    const gap = gapFraction(diameter);
    const segment = segmentFraction(diameter, segmentCount);
    const start = index * (segment + gap);
    return {start, end: start + segment};
}

export function countdownSegments(remainingSeconds, totalSeconds, diameter, segmentCount) {
    const remaining = Math.max(0, remainingSeconds);
    const fraction = totalSeconds > 0 ? Math.min(1, remaining / totalSeconds) : 0;
    if (fraction <= 0)
        return [];

    // The arc ends at the last segment's end, so the drained head moves
    // forward across that range as time passes.
    const arcRange = 1 - gapFraction(diameter);
    const fillStart = (1 - fraction) * arcRange;

    const out = [];
    for (let i = 0; i < segmentCount; i++) {
        const {start, end} = segmentRange(i, diameter, segmentCount);
        if (end > fillStart)
            out.push({start: Math.max(start, fillStart), end});
    }
    return out;
}

export function toAngle(fraction) {
    return START_ANGLE + fraction * TAU;
}

// A Cairo stroke straddles the path, so the ring's radius is inset by half
// the line width to keep the drawn band inside its own box.
export function ringRadius(diameter, lineWidth) {
    return (diameter - lineWidth) / 2;
}

export function ringCenter(diameter) {
    return diameter / 2;
}

export function percentFontSize(diameter) {
    return diameter * 0.33;
}

export function percentSignFontSize(diameter) {
    return diameter * 0.20;
}

export function countdownFontSize(diameter) {
    return diameter * 0.24;
}

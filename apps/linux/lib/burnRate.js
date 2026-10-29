// Two different quantities share the name "burn rate" on macOS.
//
// sortKey ports DashboardViewModel.burnRate (line 610): current utilisation
// over time remaining, needing only the current sample.
//
// BurnRateTracker ports BurnRateTracker.record
// (apps/macos/ClaudeDashboard/Services/BurnRateTracker.swift): a rate measured
// from the newest three samples (A, B, C), projected forward to 100%. The rules
// are in contract/README.md, "Burn-rate projection". It keeps the samples, the
// streak start and the last result per account and window in memory. Only the
// charts need the SQLite usage log, and charts are out of scope, so nothing
// here is persisted: a cold popup shows no animal until the second refresh
// tick, exactly as macOS does on launch.

import {burnRateFromRate} from './colors.js';

// Two resets_at this close (ms) are the same window.
const RESET_TOLERANCE_MS = 60000;
const MIN_GAP_S = 50;
const A_WINDOW_S = 300;
const SUSTAINED_RATE = 5;
const FIVE_HOUR_SECONDS = 18000;

function sameCycle(a, b) {
    if (a === null || a === undefined || b === null || b === undefined)
        return (a ?? null) === (b ?? null);
    return Math.abs(a - b) <= RESET_TOLERANCE_MS;
}

export class BurnRateTracker {
    constructor() {
        this._history = new Map();
    }

    reset() {
        this._history.clear();
    }

    // Returns {level, animal, projectedSeconds}, or null when there is no rate.
    record({accountId, window, utilization, resetsAtMs, recordedAtMs}) {
        const key = `${accountId}_${window}`;
        const fresh = {utilization, recordedAtMs, resetsAtMs};
        const entry = this._history.get(key);
        const c = entry?.samples[entry.samples.length - 1];

        // First measurement, a different reset cycle, or utilisation went down:
        // discard the samples and the streak.
        if (!c || !sameCycle(resetsAtMs, c.resetsAtMs) || utilization < c.utilization) {
            this._history.set(key, {samples: [fresh], streakStart: null, last: null});
            return null;
        }

        // Too close to the newest sample: skipped, not stored.
        if ((recordedAtMs - c.recordedAtMs) / 1000 < MIN_GAP_S)
            return entry.last;

        entry.samples.push(fresh);
        if (entry.samples.length > 3)
            entry.samples.shift();
        const n = entry.samples.length;
        const b = entry.samples[n - 2];
        const newest = entry.samples[n - 1];
        const ratePerMin = (from) =>
            (newest.utilization - from.utilization) * 60 / ((newest.recordedAtMs - from.recordedAtMs) / 1000);
        let rate = ratePerMin(b);
        if (n === 3) {
            const a = entry.samples[0];
            if ((newest.recordedAtMs - a.recordedAtMs) / 1000 <= A_WINDOW_S)
                rate = Math.max(rate, ratePerMin(a));
        }

        entry.streakStart = rate >= SUSTAINED_RATE ? (entry.streakStart ?? b.recordedAtMs) : null;
        const sustainedSeconds = (newest.recordedAtMs - (entry.streakStart ?? newest.recordedAtMs)) / 1000;
        entry.last = rate > 0 ? burnRateFromRate(rate, sustainedSeconds, newest.utilization) : null;
        return entry.last;
    }
}

export function sortKey(fiveHourUtilization, fiveHourResetsAtMs, nowMs, status) {
    if (status !== 'active' || fiveHourUtilization === null || fiveHourUtilization === undefined)
        return -1;
    const timeRemaining = fiveHourResetsAtMs === null || fiveHourResetsAtMs === undefined
        ? FIVE_HOUR_SECONDS
        : Math.max((fiveHourResetsAtMs - nowMs) / 1000, 60);
    return fiveHourUtilization / timeRemaining;
}

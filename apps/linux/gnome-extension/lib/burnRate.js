// Two different quantities share the name "burn rate" on macOS.
//
// sortKey ports DashboardViewModel.burnRate (line 610): current utilisation
// over time remaining, needing only the current sample.
//
// BurnRateTracker ports BurnRateTracker.record
// (apps/macos/ClaudeDashboard/Services/BurnRateTracker.swift:25): a rate
// measured between two samples, projected forward to 100%. It needs history,
// so it keeps prev/current/lastRate per account and window in memory. Only
// the charts need the SQLite usage log, and charts are out of scope, so
// nothing here is persisted — a cold popup shows no animal until the second
// refresh tick, exactly as macOS does on launch.

import {burnRateAnimal} from './colors.js';

const RATE_HOLD_SECONDS = 300;
const FIVE_HOUR_SECONDS = 18000;

export class BurnRateTracker {
    constructor() {
        this._history = new Map();
    }

    reset() {
        this._history.clear();
    }

    record({accountId, window, utilization, resetsAtMs, recordedAtMs}) {
        const key = `${accountId}_${window}`;
        const entry = this._history.get(key);
        const fresh = {utilization, recordedAtMs, resetsAtMs};

        if (!entry || !entry.current) {
            this._history.set(key, {prev: null, current: fresh, lastRate: null});
            return null;
        }

        const current = entry.current;

        if (resetsAtMs !== current.resetsAtMs || utilization < current.utilization) {
            this._history.set(key, {prev: null, current: fresh, lastRate: null});
            return null;
        }

        if (utilization > current.utilization) {
            const deltaSeconds = (recordedAtMs - current.recordedAtMs) / 1000;
            if (!(deltaSeconds > 0))
                return null;
            const rate = (utilization - current.utilization) / deltaSeconds;
            const projectedSeconds = (100 - utilization) / rate;
            this._history.set(key, {prev: current, current: fresh, lastRate: rate});
            return this._result(projectedSeconds);
        }

        // Utilisation unchanged.
        const gapSeconds = (recordedAtMs - current.recordedAtMs) / 1000;
        if (gapSeconds >= RATE_HOLD_SECONDS) {
            this._history.set(key, {prev: entry.prev, current: fresh, lastRate: null});
            return null;
        }

        if (entry.lastRate === null || !entry.prev) {
            this._history.set(key, {prev: entry.prev, current: fresh, lastRate: entry.lastRate});
            return null;
        }

        const remaining = 100 - utilization;
        this._history.set(key, {prev: entry.prev, current: fresh, lastRate: entry.lastRate});
        if (!(remaining > 0))
            return this._result(0);
        return this._result(remaining / entry.lastRate);
    }

    _result(projectedSeconds) {
        return {projectedSeconds, animal: burnRateAnimal(projectedSeconds)};
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

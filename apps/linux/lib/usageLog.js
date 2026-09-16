// Ports UsageLogStore.swift's semantics, per contract/usage-log.md.
//
// contract/usage-log.md is explicit that the SQLite file itself is NOT
// contract — "a Rust implementation may use a different engine, file layout,
// or even a different persistence mechanism entirely. What is shared is the
// column semantics and the compression policy applied on insert." GJS ships no
// SQLite binding, so this keeps rows in memory and the caller persists them as
// JSON; the column semantics and the compression policy below are reproduced
// exactly.
//
// No gi:// imports: persistence and the clock are the caller's job, so this
// runs under tests/run.js.

// UsageLogModels.swift:4-18. rawValue 2 is deliberately absent — it belonged
// to the retired Sonnet window, and contract/usage-log.md requires that old
// `w = 2` rows stay inert history rather than being reassigned to Fable.
export const WINDOW = {
    fiveHour: 0,
    sevenDay: 1,
    fable: 3,
};

export const WINDOW_LABEL = {
    0: '5h',
    1: '7d',
    3: 'F',
};

// The retired Sonnet window. Rows carrying it are read back but never written.
export const RETIRED_SONNET_WINDOW = 2;

// `t` and `rat` are Unix seconds truncated toward zero (contract/README.md's
// "Timestamps" rule: Int64(someDouble) truncates).
export function toUnixSeconds(ms) {
    return Math.trunc(ms / 1000);
}

// `u` is the one column that rounds rather than truncates:
// Int64(round(utilization * 100)), with Swift's round() rounding half away
// from zero. JS's Math.round rounds half toward +Infinity, which differs for
// negative halves — utilization is never negative in practice, but encoding it
// the same way for both signs keeps the two implementations interchangeable.
export function encodeUtilization(utilization) {
    const scaled = utilization * 100;
    return scaled < 0 ? -Math.round(-scaled) : Math.round(scaled);
}

export function decodeUtilization(u) {
    return u / 100;
}

export class UsageLog {
    constructor(rows = []) {
        this._rows = rows;
        this._nextId = rows.reduce((max, row) => Math.max(max, row.id), 0) + 1;
    }

    get rows() {
        return this._rows;
    }

    // record(accountId:window:resetsAt:utilization:isLimited:) — compression
    // runs BEFORE the insert, exactly as UsageLogStore.swift:30 does.
    record({accountId, window, resetsAtMs, utilization, isLimited = false, recordedAtMs}) {
        const rat = toUnixSeconds(resetsAtMs);
        const t = toUnixSeconds(recordedAtMs);
        const u = encodeUtilization(utilization);

        this._applyCompression(accountId, window, rat, u);
        this._rows.push({id: this._nextId++, aid: accountId, w: window, rat, t, u, lim: isLimited ? 1 : 0});
        return this._rows[this._rows.length - 1];
    }

    // applyCompression(aid:w:rat:u:) (UsageLogStore.swift:248-274): a
    // run-length "keep first and last of a plateau", scoped to an exact
    // (aid, w, rat) triple.
    //
    // All three conditions must hold or nothing is deleted: at least two
    // existing rows for that exact triple, their two `u` values equal to each
    // other, and equal to the incoming `u`. The exact-`rat` scoping is what
    // stops a real reset transition from ever being compressed away.
    _applyCompression(accountId, window, rat, u) {
        const matching = this._rows
            .filter(row => row.aid === accountId && row.w === window && row.rat === rat)
            .sort((a, b) => b.t - a.t || b.id - a.id);
        if (matching.length < 2)
            return;
        const [newest, second] = matching;
        if (newest.u !== second.u || newest.u !== u)
            return;
        // Delete the more recent of the two: once the incoming row is
        // appended, that one becomes the middle of the run.
        const index = this._rows.indexOf(newest);
        if (index >= 0)
            this._rows.splice(index, 1);
    }

    // logs(accountId:window:from:to:) — ascending by recorded time, with the
    // bounds inclusive as the SQL's `t >= ?` / `t <= ?` are.
    logs({accountId, window, fromMs = null, toMs = null}) {
        const from = fromMs === null ? null : toUnixSeconds(fromMs);
        const to = toMs === null ? null : toUnixSeconds(toMs);
        return this._rows
            .filter(row => row.aid === accountId && row.w === window &&
                (from === null || row.t >= from) &&
                (to === null || row.t <= to))
            .sort((a, b) => a.t - b.t || a.id - b.id)
            .map(toEntry);
    }

    // The all-accounts variant (UsageLogStore.swift:88) the Overview chart uses.
    allLogs({window, fromMs = null, toMs = null}) {
        const from = fromMs === null ? null : toUnixSeconds(fromMs);
        const to = toMs === null ? null : toUnixSeconds(toMs);
        return this._rows
            .filter(row => row.w === window &&
                (from === null || row.t >= from) &&
                (to === null || row.t <= to))
            .sort((a, b) => a.t - b.t || a.id - b.id)
            .map(toEntry);
    }

    // logsBefore/logsAfter (UsageLogStore.swift:185-234): the two-sample
    // buffer either side of the visible range, so a line entering or leaving
    // the viewport is drawn to the edge instead of stopping short. Both come
    // back ascending, as the Swift's re-sorted DESC query does.
    logsBefore({accountId, window, beforeMs, limit}) {
        const before = toUnixSeconds(beforeMs);
        return this._rows
            .filter(row => row.aid === accountId && row.w === window && row.t < before)
            .sort((a, b) => b.t - a.t || b.id - a.id)
            .slice(0, limit)
            .reverse()
            .map(toEntry);
    }

    logsAfter({accountId, window, afterMs, limit}) {
        const after = toUnixSeconds(afterMs);
        return this._rows
            .filter(row => row.aid === accountId && row.w === window && row.t > after)
            .sort((a, b) => a.t - b.t || a.id - b.id)
            .slice(0, limit)
            .map(toEntry);
    }

    // resetCycles(accountId:window:) (UsageLogStore.swift:128-181). Cycles are
    // NOT grouped by `rat`: for a rolling window the API returns
    // resetsAt = now + 5h on every poll, so grouping by rat would produce one
    // "cycle" per poll. A cycle boundary is a utilization drop to zero from
    // non-zero between consecutive rows.
    resetCycles({accountId, window}) {
        const rows = this._rows
            .filter(row => row.aid === accountId && row.w === window)
            .sort((a, b) => a.t - b.t || a.id - b.id);
        if (rows.length === 0)
            return [];

        const cycles = [];
        let cycleStart = rows[0].t;
        let peakU = rows[0].u;
        let count = 1;

        for (let i = 1; i < rows.length; i++) {
            const prev = rows[i - 1];
            const curr = rows[i];
            if (prev.u > 0 && curr.u === 0) {
                cycles.push({firstT: cycleStart, lastT: prev.t, peakU, count, resetAt: curr.t});
                cycleStart = curr.t;
                peakU = 0;
                // u=0 is the reset event itself; it does not count as data in
                // the new cycle.
                count = 0;
            } else {
                peakU = Math.max(peakU, curr.u);
                count += 1;
            }
        }
        const last = rows[rows.length - 1];
        cycles.push({firstT: cycleStart, lastT: last.t, peakU, count, resetAt: last.rat});

        return cycles.reverse().map(c => ({
            resetsAtMs: c.resetAt * 1000,
            firstRecordedAtMs: c.firstT * 1000,
            lastRecordedAtMs: c.lastT * 1000,
            peakUtilization: decodeUtilization(c.peakU),
            dataPointCount: c.count,
        }));
    }

    // deleteOlderThan(_:) — the retention prune.
    deleteOlderThan(cutoffMs) {
        const cutoff = toUnixSeconds(cutoffMs);
        const before = this._rows.length;
        this._rows = this._rows.filter(row => row.t >= cutoff);
        return before - this._rows.length;
    }
}

function toEntry(row) {
    return {
        id: row.id,
        accountId: row.aid,
        window: row.w,
        resetsAtMs: row.rat * 1000,
        recordedAtMs: row.t * 1000,
        utilization: decodeUtilization(row.u),
        isLimited: row.lim !== 0,
    };
}

// Array.withResetTransitions() (UsageLogModels.swift:75-138): synthetic points
// at reset boundaries so a chart line steps down instead of sloping. Synthetic
// ids count down from -1, exactly as the Swift does, so a caller can tell them
// from real rows.
export function withResetTransitions(entries) {
    if (entries.length < 2)
        return entries.slice();

    let syntheticId = -1;
    const result = [];

    const grouped = new Map();
    for (const entry of entries) {
        if (!grouped.has(entry.accountId))
            grouped.set(entry.accountId, []);
        grouped.get(entry.accountId).push(entry);
    }

    for (const accountLogs of grouped.values()) {
        const sorted = accountLogs.slice().sort((a, b) => a.recordedAtMs - b.recordedAtMs);
        for (let i = 0; i < sorted.length; i++) {
            const current = sorted[i];
            if (i > 0) {
                const prev = sorted[i - 1];
                if (prev.resetsAtMs !== current.resetsAtMs &&
                    prev.resetsAtMs > prev.recordedAtMs &&
                    prev.resetsAtMs <= current.recordedAtMs) {
                    const holdMs = prev.resetsAtMs - 1000;

                    // Point 1: hold at the old utilization, skipped when it
                    // would land on or before the previous real sample.
                    if (holdMs > prev.recordedAtMs) {
                        result.push({
                            id: syntheticId--,
                            accountId: current.accountId,
                            window: current.window,
                            resetsAtMs: prev.resetsAtMs,
                            recordedAtMs: holdMs,
                            utilization: prev.utilization,
                            isLimited: prev.isLimited,
                        });
                    }

                    // Point 2: the drop to 0%.
                    result.push({
                        id: syntheticId--,
                        accountId: current.accountId,
                        window: current.window,
                        resetsAtMs: current.resetsAtMs,
                        recordedAtMs: prev.resetsAtMs,
                        utilization: 0,
                        isLimited: false,
                    });
                }
            }
            result.push(current);
        }
    }

    result.sort((a, b) => a.recordedAtMs - b.recordedAtMs);
    return result;
}

// --- Persistence shape -------------------------------------------------
//
// The rows are handed to the caller as a plain JSON document. Bumping
// `version` is how a future change to the row shape announces itself; an
// unknown version is discarded rather than misread, because a usage log is
// regenerable history, not data worth a risky migration.
const FORMAT_VERSION = 1;

export function serialize(log) {
    return JSON.stringify({version: FORMAT_VERSION, rows: log.rows});
}

export function deserialize(text) {
    if (typeof text !== 'string' || text === '')
        return new UsageLog();
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch {
        return new UsageLog();
    }
    if (parsed?.version !== FORMAT_VERSION || !Array.isArray(parsed.rows))
        return new UsageLog();
    const rows = parsed.rows.filter(row =>
        row !== null && typeof row === 'object' &&
        typeof row.id === 'number' && typeof row.w === 'number' &&
        typeof row.rat === 'number' && typeof row.t === 'number' &&
        typeof row.u === 'number');
    return new UsageLog(rows);
}

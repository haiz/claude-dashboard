// Decodes what `claude-dashboard-helper usage` returns, per
// contract/cases/usage-decoding.json. Two traps the contract pins:
// the Fable window has no top-level field and must be found in limits[] by
// scope.model.display_name, reading `percent` rather than `utilization`; and
// `seven_day_sonnet` is a removed field that must be ignored rather than
// decoded.

import {sortKey} from './burnRate.js';

const FABLE_DISPLAY_NAME = 'Fable';

// The API emits both fractional and whole-second ISO8601. Date.parse accepts
// both forms, including six-digit fractions.
export function parseTimestamp(value) {
    if (value === null || value === undefined)
        return null;
    const ms = Date.parse(value);
    return Number.isNaN(ms) ? null : ms;
}

function window(raw) {
    return {
        utilization: raw?.utilization ?? 0,
        resetsAtMs: parseTimestamp(raw?.resets_at),
    };
}

export function fableWindow(usage) {
    const limits = Array.isArray(usage?.limits) ? usage.limits : [];
    const entry = limits.find(l => l?.scope?.model?.display_name === FABLE_DISPLAY_NAME);
    if (!entry)
        return null;
    // An entry with no percent is still a Fable window; it reads zero.
    return {
        utilization: entry.percent ?? 0,
        resetsAtMs: parseTimestamp(entry.resets_at),
    };
}

export function parseUsage(json) {
    return {
        fiveHour: window(json?.five_hour),
        sevenDay: window(json?.seven_day),
        fable: fableWindow(json),
    };
}

export function buildRows(accounts, usageByAccountId, nowMs = Date.now()) {
    const rows = accounts.map(account => {
        const raw = usageByAccountId[account.id];
        const windows = raw ? parseUsage(raw) : null;
        return {
            id: account.id,
            name: account.name,
            email: account.email ?? null,
            plan: account.plan,
            status: account.status,
            windows,
            key: sortKey(
                windows ? windows.fiveHour.utilization : null,
                windows ? windows.fiveHour.resetsAtMs : null,
                nowMs,
                windows ? account.status : 'error'),
        };
    });
    rows.sort((a, b) => b.key - a.key);
    return rows;
}

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

// contract/README.md's "Sort order" section: this is a three-tier ordering,
// not a flat sort by burn rate.
//
//   1. Pinned first, unconditionally.
//   2. The active Claude Code account next, but ONLY when nothing anywhere is
//      pinned. `anyPinned` is computed once from the whole list, not per
//      comparison — once something is pinned this tier vanishes entirely.
//   3. Burn rate descending, as the final tiebreaker.
//
// `options` carries the two pieces of state tiers 1 and 2 need. Both default
// to absent, which collapses the ordering back to tier 3 alone.
export function buildRows(accounts, usageByAccountId, nowMs = Date.now(), options = {}) {
    const pinnedId = options.pinnedId ?? null;
    const activeEmail = options.activeEmail ?? null;
    const errorsByAccountId = options.errorsByAccountId ?? {};

    const rows = accounts.map(account => {
        const raw = usageByAccountId[account.id];
        const windows = raw ? parseUsage(raw) : null;
        return {
            id: account.id,
            name: account.name,
            email: account.email ?? null,
            plan: account.plan,
            status: account.status,
            // From `list`; absent when only `decrypt` saw this account.
            storeId: account.storeId ?? null,
            source: account.source ?? null,
            chromeProfileName: account.chromeProfileName ?? null,
            windows,
            error: errorsByAccountId[account.id] ?? null,
            // Two sources, because pinning is moving. `account.isPinned` is
            // the store's own field (contract/account-schema.md) and the one
            // Linux now uses; `pinnedId` is the older per-consumer setting,
            // kept so existing callers keep working.
            isPinned: account.isPinned === true ||
                (pinnedId !== null && account.id === pinnedId),
            // macOS matches the active Claude Code account by `account.email`
            // (isActiveClaudeCodeAccount), never by name — an account with no
            // email can never be the active one.
            isActiveClaudeCode: activeEmail !== null && account.email === activeEmail,
            key: sortKey(
                windows ? windows.fiveHour.utilization : null,
                windows ? windows.fiveHour.resetsAtMs : null,
                nowMs,
                windows ? account.status : 'error'),
        };
    });

    const anyPinned = rows.some(row => row.isPinned);
    rows.sort((a, b) => {
        if (a.isPinned !== b.isPinned)
            return a.isPinned ? -1 : 1;
        if (!anyPinned && a.isActiveClaudeCode !== b.isActiveClaudeCode)
            return a.isActiveClaudeCode ? -1 : 1;
        return b.key - a.key;
    });
    return rows;
}

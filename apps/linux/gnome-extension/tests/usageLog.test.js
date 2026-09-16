// contract/usage-log.md: column semantics and the insert-time compression
// policy. The four compression cases below mirror the Swift suite the contract
// cites by name (UsageLogStoreTests.swift:37-98).

import {test, assertEqual} from './harness.js';
import {
    UsageLog, WINDOW, RETIRED_SONNET_WINDOW, encodeUtilization, decodeUtilization,
    toUnixSeconds, withResetTransitions, serialize, deserialize,
} from '../lib/usageLog.js';

const ACCOUNT = 'a@example.com';
const T0 = 1_700_000_000_000;
const RESET = T0 + 5 * 3600 * 1000;

function log() {
    return new UsageLog();
}

function record(l, {u, atMs, resetsAtMs = RESET, window = WINDOW.fiveHour, isLimited = false}) {
    return l.record({
        accountId: ACCOUNT, window, resetsAtMs, utilization: u,
        isLimited, recordedAtMs: atMs,
    });
}

// --- Column encoding ---

test('utilization is stored as percent x 100, rounded not truncated', () => {
    assertEqual(encodeUtilization(45.5), 4550);
    // The case the contract calls out: truncation would give 4500.
    assertEqual(encodeUtilization(45.005), 4501);
    assertEqual(encodeUtilization(45.999), 4600);
    assertEqual(decodeUtilization(4550), 45.5);
});

test('Swift round() rounds half away from zero, in both directions', () => {
    assertEqual(encodeUtilization(0.125), 13);
    assertEqual(encodeUtilization(-0.125), -13);
});

test('timestamps truncate toward zero rather than rounding', () => {
    assertEqual(toUnixSeconds(1999), 1);
    assertEqual(toUnixSeconds(1001), 1);
});

test('the window identifiers match UsageWindow, leaving 2 to retired Sonnet', () => {
    assertEqual(WINDOW.fiveHour, 0);
    assertEqual(WINDOW.sevenDay, 1);
    assertEqual(WINDOW.fable, 3);
    assertEqual(RETIRED_SONNET_WINDOW, 2);
});

test('the rate-limited flag is a plain 1/0 projection', () => {
    const l = log();
    record(l, {u: 10, atMs: T0, isLimited: true});
    assertEqual(l.rows[0].lim, 1);
    assertEqual(l.logs({accountId: ACCOUNT, window: WINDOW.fiveHour})[0].isLimited, true);
});

// --- Compression policy ---

test('three identical values keep only the first and the last', () => {
    const l = log();
    record(l, {u: 50, atMs: T0});
    record(l, {u: 50, atMs: T0 + 60_000});
    record(l, {u: 50, atMs: T0 + 120_000});
    assertEqual(l.rows.length, 2);
    assertEqual(l.rows[0].t, toUnixSeconds(T0));
    assertEqual(l.rows[1].t, toUnixSeconds(T0 + 120_000));
});

test('a plateau keeps compressing as it grows, not just the first triple', () => {
    const l = log();
    for (let i = 0; i < 4; i++)
        record(l, {u: 50, atMs: T0 + i * 60_000});
    assertEqual(l.rows.length, 2);
    assertEqual(l.rows[1].t, toUnixSeconds(T0 + 3 * 60_000));
});

test('distinct values are never compressed', () => {
    const l = log();
    record(l, {u: 10, atMs: T0});
    record(l, {u: 20, atMs: T0 + 60_000});
    record(l, {u: 30, atMs: T0 + 120_000});
    assertEqual(l.rows.length, 3);
});

test('a plateau then a change keeps first, last and the new value', () => {
    const l = log();
    record(l, {u: 50, atMs: T0});
    record(l, {u: 50, atMs: T0 + 60_000});
    record(l, {u: 50, atMs: T0 + 120_000});
    record(l, {u: 70, atMs: T0 + 180_000});
    assertEqual(l.rows.length, 3);
    assertEqual(l.rows.map(r => r.u).join(','), '5000,5000,7000');
});

test('compression is scoped to an exact resets_at, so a reset is never eaten', () => {
    const l = log();
    record(l, {u: 50, atMs: T0, resetsAtMs: RESET});
    record(l, {u: 50, atMs: T0 + 60_000, resetsAtMs: RESET});
    // A new cycle: same utilization, different rat. The triple no longer
    // matches, so nothing is deleted.
    record(l, {u: 50, atMs: T0 + 120_000, resetsAtMs: RESET + 3600_000});
    assertEqual(l.rows.length, 3);
});

test('compression is scoped per window and per account', () => {
    const l = log();
    record(l, {u: 50, atMs: T0, window: WINDOW.fiveHour});
    record(l, {u: 50, atMs: T0 + 60_000, window: WINDOW.sevenDay});
    record(l, {u: 50, atMs: T0 + 120_000, window: WINDOW.fable});
    assertEqual(l.rows.length, 3);
});

// --- Reads ---

test('logs come back ascending and honour inclusive bounds', () => {
    const l = log();
    record(l, {u: 10, atMs: T0});
    record(l, {u: 20, atMs: T0 + 60_000});
    record(l, {u: 30, atMs: T0 + 120_000});
    const all = l.logs({accountId: ACCOUNT, window: WINDOW.fiveHour});
    assertEqual(all.map(e => e.utilization).join(','), '10,20,30');
    const mid = l.logs({
        accountId: ACCOUNT, window: WINDOW.fiveHour,
        fromMs: T0 + 60_000, toMs: T0 + 120_000,
    });
    assertEqual(mid.length, 2);
});

test('reset cycles split on a drop to zero, not on a changing resets_at', () => {
    const l = log();
    // A rolling window reports a new resets_at on every poll; that alone must
    // not start a new cycle.
    record(l, {u: 10, atMs: T0, resetsAtMs: RESET});
    record(l, {u: 40, atMs: T0 + 60_000, resetsAtMs: RESET + 60_000});
    record(l, {u: 0, atMs: T0 + 120_000, resetsAtMs: RESET + 120_000});
    record(l, {u: 5, atMs: T0 + 180_000, resetsAtMs: RESET + 180_000});
    const cycles = l.resetCycles({accountId: ACCOUNT, window: WINDOW.fiveHour});
    assertEqual(cycles.length, 2);
    // Newest first.
    assertEqual(cycles[0].peakUtilization, 5);
    assertEqual(cycles[1].peakUtilization, 40);
    // u=0 is the reset event and is not counted as data in the new cycle.
    assertEqual(cycles[0].dataPointCount, 1);
});

test('pruning drops rows strictly older than the cutoff', () => {
    const l = log();
    record(l, {u: 10, atMs: T0});
    record(l, {u: 20, atMs: T0 + 60_000});
    assertEqual(l.deleteOlderThan(T0 + 60_000), 1);
    assertEqual(l.rows.length, 1);
});

// --- Reset transitions ---

test('a reset boundary gains a hold point and a drop to zero', () => {
    const resetsAt = T0 + 600_000;
    const entries = [
        {id: 1, accountId: ACCOUNT, window: 0, resetsAtMs: resetsAt, recordedAtMs: T0, utilization: 80, isLimited: false},
        {id: 2, accountId: ACCOUNT, window: 0, resetsAtMs: resetsAt + 3600_000, recordedAtMs: resetsAt + 60_000, utilization: 5, isLimited: false},
    ];
    const out = withResetTransitions(entries);
    assertEqual(out.length, 4);
    assertEqual(out[1].utilization, 80);
    assertEqual(out[1].recordedAtMs, resetsAt - 1000);
    assertEqual(out[2].utilization, 0);
    assertEqual(out[2].recordedAtMs, resetsAt);
    // Synthetic rows are marked by a negative id.
    assertEqual(out[1].id < 0, true);
});

test('the hold point is skipped when it would precede the previous sample', () => {
    const resetsAt = T0 + 500;
    const entries = [
        {id: 1, accountId: ACCOUNT, window: 0, resetsAtMs: resetsAt, recordedAtMs: T0, utilization: 80, isLimited: false},
        {id: 2, accountId: ACCOUNT, window: 0, resetsAtMs: resetsAt + 3600_000, recordedAtMs: resetsAt + 60_000, utilization: 5, isLimited: false},
    ];
    const out = withResetTransitions(entries);
    assertEqual(out.length, 3);
});

test('fewer than two entries pass through untouched', () => {
    assertEqual(withResetTransitions([]).length, 0);
    assertEqual(withResetTransitions([{id: 1, accountId: ACCOUNT, window: 0, resetsAtMs: 0, recordedAtMs: 0, utilization: 0, isLimited: false}]).length, 1);
});

// --- Persistence ---

test('a log survives a serialize/deserialize round trip', () => {
    const l = log();
    record(l, {u: 12.34, atMs: T0});
    record(l, {u: 56.78, atMs: T0 + 60_000});
    const back = deserialize(serialize(l));
    assertEqual(back.rows.length, 2);
    assertEqual(back.logs({accountId: ACCOUNT, window: WINDOW.fiveHour})[1].utilization, 56.78);
});

test('a missing, malformed or unknown-version document reads as an empty log', () => {
    assertEqual(deserialize('').rows.length, 0);
    assertEqual(deserialize('not json').rows.length, 0);
    assertEqual(deserialize('{"version":99,"rows":[{"id":1}]}').rows.length, 0);
    assertEqual(deserialize('{"version":1,"rows":"nope"}').rows.length, 0);
});

test('ids keep climbing after a reload rather than colliding', () => {
    const l = log();
    record(l, {u: 10, atMs: T0});
    const back = deserialize(serialize(l));
    const added = back.record({
        accountId: ACCOUNT, window: WINDOW.fiveHour, resetsAtMs: RESET,
        utilization: 20, recordedAtMs: T0 + 60_000,
    });
    assertEqual(added.id > l.rows[0].id, true);
});

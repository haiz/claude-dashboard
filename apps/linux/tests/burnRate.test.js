import GLib from 'gi://GLib';
import {test, assertEqual, assertClose} from './harness.js';
import {BurnRateTracker, sortKey} from '../lib/burnRate.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

const RESET = Date.parse('2026-09-11T18:00:00Z');
const T0 = Date.parse('2026-09-11T12:00:00Z');

function sample(t, utilization, resetsAtMs = RESET) {
    return {accountId: 'a', window: 'five_hour', utilization, resetsAtMs, recordedAtMs: t};
}

test('tracker cases from the contract', () => {
    for (const c of loadCases('burn-rate-tracker.json').cases) {
        const tracker = new BurnRateTracker();
        c.steps.forEach((s, i) => {
            const r = tracker.record({
                accountId: 'a',
                window: 'five_hour',
                utilization: s.utilization,
                resetsAtMs: s.resets_at === null ? null : s.resets_at * 1000,
                recordedAtMs: s.at * 1000,
            });
            const label = `${c.name} step ${i}`;
            assertEqual(r?.level ?? null, s.expect_level, label);
            if (s.expect_level !== null && s.expect_projected_seconds !== undefined)
                assertClose(r.projectedSeconds, s.expect_projected_seconds, 1e-6, label);
        });
    }
});

test('a rising utilisation keeps the animal field', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    const r = tracker.record(sample(T0 + 600000, 20));
    assertClose(r.projectedSeconds, 4800, 1e-6);
    assertEqual(r.animal, '🐎');
});

test('windows are tracked independently', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    assertEqual(tracker.record({...sample(T0 + 600000, 20), window: 'seven_day'}), null);
});

test('the sort key is utilisation over time remaining', () => {
    const now = T0;
    assertClose(sortKey(50, now + 3600000, now, 'active'), 50 / 3600, 1e-12);
});

test('a reset time in the past still floors at sixty seconds', () => {
    const now = T0;
    assertClose(sortKey(50, now - 1000, now, 'active'), 50 / 60, 1e-12);
});

test('a missing reset time assumes a whole five-hour window', () => {
    assertClose(sortKey(50, null, T0, 'active'), 50 / 18000, 1e-12);
});

test('inactive accounts sort last', () => {
    assertEqual(sortKey(90, T0 + 3600000, T0, 'expired'), -1);
    assertEqual(sortKey(90, T0 + 3600000, T0, 'error'), -1);
});

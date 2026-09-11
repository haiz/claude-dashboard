import {test, assertEqual, assertClose} from './harness.js';
import {BurnRateTracker, sortKey} from '../lib/burnRate.js';

const RESET = Date.parse('2026-09-11T18:00:00Z');
const T0 = Date.parse('2026-09-11T12:00:00Z');

function sample(t, utilization, resetsAtMs = RESET) {
    return {accountId: 'a', window: 'five_hour', utilization, resetsAtMs, recordedAtMs: t};
}

test('the first sample yields no projection', () => {
    const tracker = new BurnRateTracker();
    assertEqual(tracker.record(sample(T0, 10)), null);
});

test('a rising utilisation projects to one hundred percent', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    // +10 points over 600 s is 1 point per 60 seconds, so 80 points remain: 4800 s.
    const r = tracker.record(sample(T0 + 600000, 20));
    assertClose(r.projectedSeconds, 4800, 1e-6);
    assertEqual(r.animal, '🐎');
});

test('a new reset cycle discards the history', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    assertEqual(tracker.record(sample(T0 + 600000, 20, RESET + 18000000)), null);
});

test('a utilisation that went down discards the history', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 40));
    assertEqual(tracker.record(sample(T0 + 600000, 30)), null);
});

test('an unchanged utilisation reuses the last rate within five minutes', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    tracker.record(sample(T0 + 600000, 20));
    const r = tracker.record(sample(T0 + 600000 + 60000, 20));
    assertClose(r.projectedSeconds, 4800, 1e-6);
});

test('an unchanged utilisation drops the rate after five minutes', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 10));
    tracker.record(sample(T0 + 600000, 20));
    assertEqual(tracker.record(sample(T0 + 600000 + 300000, 20)), null);
});

test('an exhausted window projects to zero', () => {
    const tracker = new BurnRateTracker();
    tracker.record(sample(T0, 90));
    tracker.record(sample(T0 + 600000, 100));
    const r = tracker.record(sample(T0 + 600000 + 60000, 100));
    assertEqual(r.projectedSeconds, 0);
    assertEqual(r.animal, '🐆');
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

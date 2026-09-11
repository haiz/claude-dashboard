import {test, assertEqual} from './harness.js';
import {formattedCountdown, formatResetTime} from '../lib/format.js';

const FIVE_HOURS = 18000;
const SEVEN_DAYS = 604800;

test('five-hour countdown shows hours and minutes above an hour', () => {
    assertEqual(formattedCountdown(3 * 3600 + 7 * 60 + 12, FIVE_HOURS), '3:07');
});

test('five-hour countdown shows minutes and seconds below an hour', () => {
    assertEqual(formattedCountdown(7 * 60 + 5, FIVE_HOURS), '7:05');
});

test('seven-day countdown shows days and hours above a day', () => {
    assertEqual(formattedCountdown(5 * 86400 + 8 * 3600, SEVEN_DAYS), '5d8h');
});

test('seven-day countdown falls back to hours and minutes under a day', () => {
    assertEqual(formattedCountdown(3 * 3600 + 4 * 60, SEVEN_DAYS), '3:04');
});

test('an elapsed window reads zero', () => {
    assertEqual(formattedCountdown(0, FIVE_HOURS), '0:00');
    assertEqual(formattedCountdown(-10, SEVEN_DAYS), '0:00');
});

test('a passed reset time reads now', () => {
    const now = new Date('2026-09-11T12:00:00Z');
    assertEqual(formatResetTime(new Date('2026-09-11T11:00:00Z'), FIVE_HOURS, now), 'now');
});

test('the seven-day label rounds to the nearest ten minutes and drops a zero minute', () => {
    const now = new Date('2026-09-11T12:00:00');
    // 23:59 local rounds up to the next midnight, so the day must roll over too.
    const resets = new Date('2026-09-12T23:59:00');
    assertEqual(formatResetTime(resets, SEVEN_DAYS, now), 'Sun 12am');
});

test('a rounded-to-the-hour seven-day label drops the minutes', () => {
    const now = new Date('2026-09-11T12:00:00');
    const resets = new Date('2026-09-13T13:04:00');
    assertEqual(formatResetTime(resets, SEVEN_DAYS, now), 'Sun 1pm');
});

test('the seven-day label keeps a non-zero rounded minute', () => {
    const now = new Date('2026-09-11T12:00:00');
    const resets = new Date('2026-09-13T13:07:00');
    assertEqual(formatResetTime(resets, SEVEN_DAYS, now), 'Sun 1:10pm');
});

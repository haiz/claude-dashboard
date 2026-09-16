import {test, assertEqual} from './harness.js';
import {
    isNewer, versionFromTag, releaseIfNewer, shouldCheck, CHECK_INTERVAL_MS,
} from '../lib/update.js';

test('a higher component anywhere makes a release newer', () => {
    assertEqual(isNewer('1.18.0', '1.17.2'), true);
    assertEqual(isNewer('2.0.0', '1.99.99'), true);
    assertEqual(isNewer('1.17.3', '1.17.2'), true);
});

test('the same or an older version is not newer', () => {
    assertEqual(isNewer('1.17.2', '1.17.2'), false);
    assertEqual(isNewer('1.17.1', '1.17.2'), false);
    assertEqual(isNewer('1.16.9', '1.17.0'), false);
});

test('a missing component reads as zero, not as smaller', () => {
    assertEqual(isNewer('1.18', '1.17.2'), true);
    assertEqual(isNewer('1.17', '1.17.0'), false);
    assertEqual(isNewer('1.17.0', '1.17'), false);
});

test('components that are not numbers are dropped rather than throwing', () => {
    assertEqual(isNewer('1.18.0-beta', '1.17.2'), true);
    assertEqual(isNewer('', '1.0.0'), false);
    assertEqual(isNewer(undefined, '1.0.0'), false);
});

test('a leading v is stripped from the tag and nothing else is', () => {
    assertEqual(versionFromTag('v1.17.2'), '1.17.2');
    assertEqual(versionFromTag('1.17.2'), '1.17.2');
    assertEqual(versionFromTag('release-1.17.2'), 'release-1.17.2');
});

test('a newer release comes back with its version and page', () => {
    const release = releaseIfNewer({
        tag_name: 'v1.18.0',
        html_url: 'https://example.invalid/r/1.18.0',
        body: 'notes',
    }, '1.17.2');
    assertEqual(release.version, '1.18.0');
    assertEqual(release.url, 'https://example.invalid/r/1.18.0');
    assertEqual(release.body, 'notes');
});

test('an older or equal release comes back as null, not as an error', () => {
    assertEqual(releaseIfNewer({tag_name: 'v1.17.2'}, '1.17.2'), null);
    assertEqual(releaseIfNewer({tag_name: 'v1.0.0'}, '1.17.2'), null);
});

test('a release with no html_url falls back to a derived tag URL', () => {
    const release = releaseIfNewer({tag_name: 'v9.0.0'}, '1.0.0');
    assertEqual(release.url.endsWith('/releases/tag/v9.0.0'), true);
});

test('a payload that is not a release throws rather than reading as up to date', () => {
    // "could not tell" must be distinguishable from "you are current".
    for (const bad of [null, 'nope', {}, {tag_name: 5}]) {
        let threw = false;
        try {
            releaseIfNewer(bad, '1.0.0');
        } catch {
            threw = true;
        }
        assertEqual(threw, true);
    }
});

test('a never-checked install is always due', () => {
    assertEqual(shouldCheck({lastCheckMs: 0, nowMs: 1_000_000}), true);
    assertEqual(shouldCheck({lastCheckMs: NaN, nowMs: 1_000_000}), true);
});

test('the check is rate limited to once a day', () => {
    const now = 1_700_000_000_000;
    assertEqual(shouldCheck({lastCheckMs: now - 1000, nowMs: now}), false);
    assertEqual(shouldCheck({lastCheckMs: now - CHECK_INTERVAL_MS, nowMs: now}), true);
    assertEqual(shouldCheck({lastCheckMs: now - CHECK_INTERVAL_MS - 1, nowMs: now}), true);
});

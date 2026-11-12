import GLib from 'gi://GLib';
import {test, assertEqual, assertDeepEqual} from './harness.js';
import {fieldName, parseGrants, summarize, URGENT_MS} from '../lib/limitResets.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

function shape(s) {
    return s === null ? null : {
        count: s.count,
        expires_at_epoch: Math.floor(s.expiresAtMs / 1000),
        urgent: s.urgent,
    };
}

test('limit resets match every contract case', () => {
    const cases = loadCases('limit-resets.json');
    assertEqual(cases.length, 27, 'case count');
    for (const c of cases) {
        const got = summarize(parseGrants(c.input), Date.parse(c.now));
        assertDeepEqual({full: shape(got.full), fable: shape(got.fable)}, c.expect, c.name);
    }
});

test('fieldName decodes the base64 constant at runtime', () => {
    const n = fieldName();
    assertEqual(n.length, 11, 'length');
    assertEqual(n.includes('_'), true, 'underscore');
});

test('urgent threshold is seven days', () => {
    assertEqual(URGENT_MS, 604800000, 'ms');
});

test('resets_left is capped at u32 max when summed', () => {
    const g = n => ({resets_left: n, ends_at: '2099-01-01T00:00:00Z', gives_back_weekly: true});
    const s = summarize(parseGrants({eligible: true, grants: [g(4294967295), g(5)]}), 0);
    assertEqual(s.fable.count, 4294967295, 'saturated');
    assertEqual(parseGrants({eligible: true, grants: [g(4294967296)]}).length, 0, 'over u32');
});

test('a grant ending within the current second of now is expired (whole seconds)', () => {
    const grants = parseGrants({eligible: true, grants: [
        {resets_left: 1, ends_at: '2026-11-12T00:00:00.500+00:00', gives_back_weekly: true},
    ]});
    const got = summarize(grants, Date.parse('2026-11-12T00:00:00.000Z'));
    assertEqual(got.fable, null, 'fable dropped');
});

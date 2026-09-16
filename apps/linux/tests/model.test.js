import GLib from 'gi://GLib';
import {test, assertEqual} from './harness.js';
import {parseUsage, buildRows} from '../lib/model.js';
import {parseAccounts} from '../lib/helper.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

function epochOrNull(ms) {
    return ms === null ? null : Math.floor(ms / 1000);
}

test('usage decoding matches every contract case', () => {
    for (const c of loadCases('usage-decoding.json')) {
        const u = parseUsage(c.input);
        const e = c.expect;
        assertEqual(u.fiveHour.utilization, e.five_hour_utilization, `${c.name}: 5h utilisation`);
        assertEqual(epochOrNull(u.fiveHour.resetsAtMs), e.five_hour_resets_at_epoch, `${c.name}: 5h reset`);
        assertEqual(u.sevenDay.utilization, e.seven_day_utilization, `${c.name}: 7d utilisation`);
        assertEqual(epochOrNull(u.sevenDay.resetsAtMs), e.seven_day_resets_at_epoch, `${c.name}: 7d reset`);
        assertEqual(u.fable !== null, e.fable_present, `${c.name}: fable present`);
        if (e.fable_present) {
            assertEqual(u.fable.utilization, e.fable_utilization, `${c.name}: fable utilisation`);
            assertEqual(epochOrNull(u.fable.resetsAtMs), e.fable_resets_at_epoch, `${c.name}: fable reset`);
        }
    }
});

test('rows sort by descending burn rate with inactive accounts last', () => {
    const now = Date.parse('2026-09-11T12:00:00Z');
    const accounts = [
        {id: 'slow', name: 'slow', plan: 'max', status: 'active'},
        {id: 'dead', name: 'dead', plan: 'pro', status: 'expired'},
        {id: 'fast', name: 'fast', plan: 'max', status: 'active'},
    ];
    const usage = {
        slow: {five_hour: {utilization: 10, resets_at: '2026-09-11T16:00:00Z'}, seven_day: {utilization: 0, resets_at: null}},
        fast: {five_hour: {utilization: 90, resets_at: '2026-09-11T13:00:00Z'}, seven_day: {utilization: 0, resets_at: null}},
    };
    const rows = buildRows(accounts, usage, now);
    assertEqual(rows.map(r => r.id).join(','), 'fast,slow,dead');
});

test('an account with no usage keeps its row and reports the failure', () => {
    const now = Date.parse('2026-09-11T12:00:00Z');
    const rows = buildRows([{id: 'a', name: 'a', plan: 'pro', status: 'active'}], {}, now);
    assertEqual(rows.length, 1);
    assertEqual(rows[0].windows, null);
});

// Regression for the whole-branch review's CRITICAL 1: every account object
// up to this point in the suite is a hand-rolled literal that already
// carries a convenient "id" the real helper never emits. Piping two real,
// id-less decrypt-shaped accounts through parseAccounts and into buildRows
// is what actually exercises the identity derivation end to end — with the
// old bug (account.id read directly off the helper's output) both accounts
// resolve to the same undefined id, this test fails loudly: the two rows
// collapse onto one usageByAccountId entry and one account's utilisation
// leaks onto the other's row.
test('two distinct accounts keep separate identities end-to-end through parseAccounts and buildRows', () => {
    const now = Date.parse('2026-09-11T12:00:00Z');
    const decrypted = JSON.stringify([
        {name: 'Alex Rivera', email: 'alex@example.com', orgId: 'org-a', sessionKey: 'sk-a', plan: 'Pro', status: 'active'},
        {name: 'Bailey Chen', email: 'bailey@example.com', orgId: 'org-b', sessionKey: 'sk-b', plan: 'Max', status: 'active'},
    ]);
    const accounts = parseAccounts(decrypted);
    assertEqual(accounts.length, 2);
    assertEqual(accounts[0].id === accounts[1].id, false);

    const usageByAccountId = {
        [accounts[0].id]: {five_hour: {utilization: 10, resets_at: null}, seven_day: {utilization: 0, resets_at: null}},
        [accounts[1].id]: {five_hour: {utilization: 90, resets_at: null}, seven_day: {utilization: 0, resets_at: null}},
    };
    const rows = buildRows(accounts, usageByAccountId, now);
    assertEqual(rows.length, 2);

    const alex = rows.find(r => r.email === 'alex@example.com');
    const bailey = rows.find(r => r.email === 'bailey@example.com');
    assertEqual(alex.windows.fiveHour.utilization, 10);
    assertEqual(bailey.windows.fiveHour.utilization, 90);
    assertEqual(alex.plan, 'Pro');
    assertEqual(bailey.plan, 'Max');
});

// contract/README.md's "Sort order" section: the three-tier ordering
// buildRows must reproduce. Tier 3 (burn rate) already has coverage in
// model.test.js; these pin tiers 1 and 2 and, critically, the rule that tier 2
// disappears the moment anything is pinned.

import {test, assertEqual} from './harness.js';
import {buildRows} from '../lib/model.js';
import {activeEmailFrom} from '../lib/claudeCode.js';
import {planBadgeColor} from '../lib/plan.js';

const NOW = 1_700_000_000_000;

// Utilization drives tier 3, so these are ordered low -> high burn rate:
// "cold" sorts last, "hot" first, whenever tiers 1 and 2 do not intervene.
function account(id, {email = `${id}@example.com`} = {}) {
    return {id, name: id, email, plan: 'Max', status: 'active'};
}

function usage(utilization) {
    return {
        five_hour: {utilization, resets_at: new Date(NOW + 3_600_000).toISOString()},
        seven_day: {utilization, resets_at: new Date(NOW + 86_400_000).toISOString()},
    };
}

const ACCOUNTS = [account('cold'), account('warm'), account('hot')];
const USAGE = {cold: usage(5), warm: usage(40), hot: usage(90)};

function order(options) {
    return buildRows(ACCOUNTS, USAGE, NOW, options).map(r => r.id);
}

test('with no pin and no Claude Code account, rows sort by burn rate alone', () => {
    assertEqual(order({}).join(','), 'hot,warm,cold');
});

test('a pinned account sorts first regardless of burn rate', () => {
    assertEqual(order({pinnedId: 'cold'}).join(','), 'cold,hot,warm');
});

test('the active Claude Code account sorts first when nothing is pinned', () => {
    assertEqual(order({activeEmail: 'cold@example.com'}).join(','), 'cold,hot,warm');
});

test('tier 2 is skipped entirely once any account is pinned', () => {
    // `warm` is pinned, so `cold` being the active Claude Code account must
    // NOT lift it above `hot`: anyPinned is computed from the whole list.
    assertEqual(
        order({pinnedId: 'warm', activeEmail: 'cold@example.com'}).join(','),
        'warm,hot,cold');
});

test('pin and Claude Code flags land on the rows themselves', () => {
    const rows = buildRows(ACCOUNTS, USAGE, NOW, {
        pinnedId: 'warm',
        activeEmail: 'cold@example.com',
    });
    const byId = Object.fromEntries(rows.map(r => [r.id, r]));
    assertEqual(byId.warm.isPinned, true);
    assertEqual(byId.cold.isPinned, false);
    assertEqual(byId.cold.isActiveClaudeCode, true);
    assertEqual(byId.hot.isActiveClaudeCode, false);
});

test('an account with no email can never be the active Claude Code account', () => {
    // A null email must not match a null activeEmail, and must not match the
    // literal string either.
    const rows = buildRows([{id: 'x', name: 'x', email: null, plan: 'Max', status: 'active'}],
        {}, NOW, {activeEmail: null});
    assertEqual(rows[0].isActiveClaudeCode, false);
});

test('per-account fetch errors land on their own row only', () => {
    const rows = buildRows(ACCOUNTS, USAGE, NOW, {errorsByAccountId: {warm: 'boom'}});
    const byId = Object.fromEntries(rows.map(r => [r.id, r]));
    assertEqual(byId.warm.error, 'boom');
    assertEqual(byId.hot.error, null);
});

// --- ClaudeCodeAccountDetector ---

test('the active Claude Code email comes from oauthAccount.emailAddress', () => {
    assertEqual(
        activeEmailFrom('{"oauthAccount":{"emailAddress":"a@b.co"}}'),
        'a@b.co');
});

test('a missing, malformed or empty ~/.claude.json yields no active account', () => {
    assertEqual(activeEmailFrom(''), null);
    assertEqual(activeEmailFrom('not json'), null);
    assertEqual(activeEmailFrom('{}'), null);
    assertEqual(activeEmailFrom('{"oauthAccount":{}}'), null);
    assertEqual(activeEmailFrom('{"oauthAccount":{"emailAddress":"   "}}'), null);
    assertEqual(activeEmailFrom('{"oauthAccount":{"emailAddress":42}}'), null);
    assertEqual(activeEmailFrom(null), null);
});

test('the email is trimmed, matching how buildRows compares it', () => {
    assertEqual(activeEmailFrom('{"oauthAccount":{"emailAddress":" a@b.co "}}'), 'a@b.co');
});

// --- AccountBadgeColor ---

test('each plan gets its own badge tint', () => {
    const pro = planBadgeColor('Pro');
    const max5 = planBadgeColor('Max 5x');
    const max20 = planBadgeColor('Max 20x');
    assertEqual(pro === max5, false);
    assertEqual(max5 === max20, false);
    // macOS maps max200 ("Max") and max5x to the same .purple.
    assertEqual(planBadgeColor('Max'), max5);
});

test('an unknown plan string still gets a tint rather than none', () => {
    assertEqual(planBadgeColor('Max 900x'), planBadgeColor('Max'));
    assertEqual(planBadgeColor(undefined), planBadgeColor('Max'));
});

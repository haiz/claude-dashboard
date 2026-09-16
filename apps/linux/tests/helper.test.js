import GLib from 'gi://GLib';
import {test, assertEqual, assertDeepEqual} from './harness.js';
import {decryptArgv, usageArgv, parseAccounts, parseUsagePayload, isNoAccountsMessage} from '../lib/helper.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

test('the decrypt invocation matches what the bash CLI runs', () => {
    assertDeepEqual(decryptArgv('/bin/helper'), ['/bin/helper', 'decrypt']);
});

test('the usage invocation passes the org id and session key positionally', () => {
    assertDeepEqual(usageArgv('/bin/helper', 'org-1', 'sk-abc'),
        ['/bin/helper', 'usage', 'org-1', 'sk-abc']);
});

test('an empty helper store parses as no accounts', () => {
    assertEqual(isNoAccountsMessage('No accounts found. Run: claude-dashboard-cli sync\n'), true);
    assertEqual(isNoAccountsMessage('No active accounts with session keys found.\n'), true);
    assertEqual(isNoAccountsMessage('[]'), false);
    assertEqual(isNoAccountsMessage(''), false);
});

test('account JSON parses into an array', () => {
    // The real helper never emits an "id" field (contract/helper-cli.md's
    // decrypt section: six keys, always) — this fixture matches that shape.
    const out = parseAccounts('[{"name":"me","email":null,"orgId":"org-1","sessionKey":null,"plan":"Pro","status":"active"}]');
    assertEqual(out.length, 1);
    assertEqual(out[0].name, 'me');
});

test('malformed account JSON yields an empty array rather than throwing', () => {
    assertDeepEqual(parseAccounts('not json'), []);
});

test('a non-array payload yields an empty array', () => {
    assertDeepEqual(parseAccounts('{"name":"A"}'), []);
});

test('parseAccounts derives a stable id from email, preferring it over name', () => {
    const [out] = parseAccounts(JSON.stringify([
        {name: 'Jordan Casey', email: 'jordan@example.com', orgId: 'org-1', sessionKey: 'sk', plan: 'Pro', status: 'active'},
    ]));
    assertEqual(out.id, 'jordan@example.com');
});

test('parseAccounts falls back to name when email is null', () => {
    const [out] = parseAccounts(JSON.stringify([
        {name: 'Jordan Casey', email: null, orgId: 'org-1', sessionKey: 'sk', plan: 'Pro', status: 'active'},
    ]));
    assertEqual(out.id, 'Jordan Casey');
});

test('parseAccounts never derives orgId as an identity', () => {
    // contract/README.md's "Account identity" section: an orgId identifies an
    // organisation, not an account — every member of a company org shares
    // one, so two colleagues with no email/name must not collide on orgId.
    const [a, b] = parseAccounts(JSON.stringify([
        {name: null, email: null, orgId: 'org-shared', sessionKey: 'sk-a', plan: 'Pro', status: 'active'},
        {name: null, email: null, orgId: 'org-shared', sessionKey: 'sk-b', plan: 'Pro', status: 'active'},
    ]));
    assertEqual(a.id.includes('org-shared'), false);
    assertEqual(b.id.includes('org-shared'), false);
});

test('parseAccounts derives a total, non-undefined id when both email and name are missing', () => {
    const [out] = parseAccounts(JSON.stringify([
        {name: null, email: null, orgId: 'org-1', sessionKey: 'sk', plan: 'Pro', status: 'active'},
    ]));
    assertEqual(typeof out.id, 'string');
    assertEqual(out.id.length > 0, true);
});

test('the real decrypt projection parses into a usable identity with no id of its own', () => {
    for (const c of loadCases('decrypt-projection.json')) {
        // Pin the shape itself: exactly six keys, no id, matching
        // contract/helper-cli.md's "six fields means six keys, always".
        assertDeepEqual(Object.keys(c.account).sort(),
            ['email', 'name', 'orgId', 'plan', 'sessionKey', 'status'], `${c.name}: projection shape`);
        assertEqual('id' in c.account, false, `${c.name}: the real helper never emits an id`);

        const [parsed] = parseAccounts(JSON.stringify([c.account]));
        assertEqual(parsed.id, c.account.email, `${c.name}: email is preferred as identity`);
        // The plan string reaches straight through, unmapped — C2's fix.
        assertEqual(parsed.plan, c.account.plan, `${c.name}: plan string passes through unchanged`);
    }
});

test('valid usage JSON parses into an object', () => {
    const out = parseUsagePayload('{"five_hour":{"utilization":42}}');
    assertEqual(out.five_hour.utilization, 42);
});

test('malformed usage JSON yields null rather than throwing', () => {
    assertEqual(parseUsagePayload('not json'), null);
});

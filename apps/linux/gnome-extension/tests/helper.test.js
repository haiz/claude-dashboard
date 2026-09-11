import {test, assertEqual, assertDeepEqual} from './harness.js';
import {decryptArgv, usageArgv, parseAccounts, isNoAccountsMessage} from '../lib/helper.js';

test('the decrypt invocation matches what the bash CLI runs', () => {
    assertDeepEqual(decryptArgv('/bin/helper'), ['/bin/helper', 'decrypt']);
});

test('the usage invocation passes the org id and session key positionally', () => {
    assertDeepEqual(usageArgv('/bin/helper', 'org-1', 'sk-abc'),
        ['/bin/helper', 'usage', 'org-1', 'sk-abc']);
});

test('an empty helper store parses as no accounts', () => {
    assertEqual(isNoAccountsMessage('No accounts found. Run: claude-dashboard-cli sync\n'), true);
    assertEqual(isNoAccountsMessage('[]'), false);
});

test('account JSON parses into an array', () => {
    const out = parseAccounts('[{"id":"A","name":"me","plan":"max","status":"active"}]');
    assertEqual(out.length, 1);
    assertEqual(out[0].id, 'A');
});

test('malformed account JSON yields an empty array rather than throwing', () => {
    assertDeepEqual(parseAccounts('not json'), []);
});

test('a non-array payload yields an empty array', () => {
    assertDeepEqual(parseAccounts('{"id":"A"}'), []);
});

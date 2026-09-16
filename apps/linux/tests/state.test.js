import GLib from 'gi://GLib';
import {test, assertEqual} from './harness.js';
import {STATE_SCHEMA_VERSION, parseState, isStale, toBuildRowsInput} from '../lib/state.js';

const REPO = GLib.getenv('CLAUDE_DASHBOARD_REPO') ?? '../..';

function loadCases(name) {
    const [ok, bytes] = GLib.file_get_contents(`${REPO}/contract/cases/${name}`);
    if (!ok)
        throw new Error(`cannot read contract case ${name}`);
    return JSON.parse(new TextDecoder().decode(bytes));
}

test('the reader implements schema version 1', () => {
    assertEqual(STATE_SCHEMA_VERSION, 1);
});

test('every contract case parses as the fixture says it must', () => {
    for (const c of loadCases('linux-state.json')) {
        const state = parseState(c.text);
        assertEqual(state !== null, c.expect_parsed, `${c.name}:`);
        if (!c.expect_parsed)
            continue;
        assertEqual(state.accounts.length, c.expect_account_count, `${c.name}: accounts`);
        assertEqual(toBuildRowsInput(state).activeEmail, c.expect_active_email, `${c.name}: activeEmail`);
    }
});

test('a snapshot inside three intervals is fresh', () => {
    const state = parseState('{"schemaVersion":1,"polledAtMs":1000000,"daemon":{"intervalSeconds":120}}');
    assertEqual(isStale(state, 1000000 + 359 * 1000), false);
});

test('a snapshot past three intervals is stale', () => {
    const state = parseState('{"schemaVersion":1,"polledAtMs":1000000,"daemon":{"intervalSeconds":120}}');
    assertEqual(isStale(state, 1000000 + 361 * 1000), true);
});

test('a short interval cannot push the staleness floor below five minutes', () => {
    const state = parseState('{"schemaVersion":1,"polledAtMs":1000000,"daemon":{"intervalSeconds":10}}');
    assertEqual(isStale(state, 1000000 + 299 * 1000), false);
    assertEqual(isStale(state, 1000000 + 301 * 1000), true);
});

test('toBuildRowsInput hands buildRows the verbatim usage payload', () => {
    const state = parseState(loadCases('linux-state.json')[0].text);
    const input = toBuildRowsInput(state);
    assertEqual(input.usageByAccountId['acc-1'].five_hour.utilization, 5);
    assertEqual(input.accounts[0].id, 'acc-1');
});

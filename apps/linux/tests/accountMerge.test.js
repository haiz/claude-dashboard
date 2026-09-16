// `list` + `decrypt` are two views of one store; the merge is what makes an
// expired account visible while still polling the active ones.

import {test, assertEqual} from './harness.js';
import {parseListedAccounts, mergeAccounts, listArgv, parseAccounts} from '../lib/helper.js';
import {buildRows} from '../lib/model.js';

const LISTED = JSON.stringify([
    {id: 'store-1', name: 'a@example.com', email: 'a@example.com', orgId: 'org-a',
     chromeProfileName: 'Profile 1', browser: 'chrome', plan: 'Max 5x',
     status: 'active', source: 'browser', isPinned: false, lastSynced: 1},
    {id: 'store-2', name: 'b@example.com', email: 'b@example.com', orgId: null,
     chromeProfileName: null, browser: 'brave', plan: 'Pro',
     status: 'expired', source: 'manual', isPinned: false, lastSynced: null},
]);

const DECRYPTED = JSON.stringify([
    {name: 'a@example.com', email: 'a@example.com', orgId: 'org-a',
     sessionKey: 'sk-a', plan: 'Max 5x', status: 'active'},
]);

test('the list invocation is a bare subcommand', () => {
    assertEqual(listArgv('/bin/helper').join(' '), '/bin/helper list');
});

test('listed accounts keep the derived id and carry the store id alongside', () => {
    const listed = parseListedAccounts(LISTED);
    // The derived id is what every other surface already keys on, so it stays
    // the row id; the store id rides along for `remove`.
    assertEqual(listed[0].id, 'a@example.com');
    assertEqual(listed[0].storeId, 'store-1');
});

test('malformed or non-array list output parses as no accounts', () => {
    assertEqual(parseListedAccounts('nope').length, 0);
    assertEqual(parseListedAccounts('{"a":1}').length, 0);
    assertEqual(parseListedAccounts('[null, 3]').length, 0);
});

test('the merge keeps the expired account decrypt filtered out', () => {
    const merged = mergeAccounts(parseListedAccounts(LISTED), parseAccounts(DECRYPTED));
    assertEqual(merged.length, 2);
    const expired = merged.find(a => a.id === 'b@example.com');
    assertEqual(expired.status, 'expired');
    assertEqual(expired.source, 'manual');
});

test('the merge gives the active account its session key', () => {
    const merged = mergeAccounts(parseListedAccounts(LISTED), parseAccounts(DECRYPTED));
    const active = merged.find(a => a.id === 'a@example.com');
    assertEqual(active.sessionKey, 'sk-a');
    // decrypt carries no storeId, and must not wipe the one list supplied.
    assertEqual(active.storeId, 'store-1');
    assertEqual(active.chromeProfileName, 'Profile 1');
});

test('an account seen only by decrypt is still kept, with no store id', () => {
    const merged = mergeAccounts([], parseAccounts(DECRYPTED));
    assertEqual(merged.length, 1);
    assertEqual(merged[0].storeId, null);
});

test('an all-expired store yields rows rather than an empty panel', () => {
    const merged = mergeAccounts(parseListedAccounts(LISTED), []);
    assertEqual(merged.length, 2);
});

test('the fields the expired card branches on reach the row', () => {
    const merged = mergeAccounts(parseListedAccounts(LISTED), parseAccounts(DECRYPTED));
    const rows = buildRows(merged, {}, Date.now());
    const expired = rows.find(r => r.id === 'b@example.com');
    assertEqual(expired.source, 'manual');
    assertEqual(expired.chromeProfileName, null);
    assertEqual(expired.storeId, 'store-2');
    const active = rows.find(r => r.id === 'a@example.com');
    assertEqual(active.chromeProfileName, 'Profile 1');
});

test('a row built from decrypt alone reports those fields as absent', () => {
    const rows = buildRows(parseAccounts(DECRYPTED), {}, Date.now());
    assertEqual(rows[0].source, null);
    assertEqual(rows[0].chromeProfileName, null);
    assertEqual(rows[0].storeId, null);
});

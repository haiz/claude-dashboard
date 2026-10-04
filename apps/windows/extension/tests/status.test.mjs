import { test } from 'node:test';
import assert from 'node:assert/strict';
import { describeResult } from '../lib/status.js';

test('a successful sync shows the email and no badge', () => {
  const s = describeResult({ ok: true, email: 'a@x.com' });
  assert.equal(s.badge, '');
  assert.match(s.text, /a@x\.com/);
});

test('unreachable_host_sets_badge_and_status', () => {
  // sendNativeMessage rejects when the host is not registered / app not installed.
  const s = describeResult(new Error('Specified native messaging host not found.'));
  assert.equal(s.badge, '!');
  assert.match(s.text, /install|not found|Claude Dashboard/i);
});

test('a host error reply surfaces its message', () => {
  const s = describeResult({ ok: false, error: 'muted', message: 'This account was removed.' });
  assert.equal(s.badge, '!');
  assert.match(s.text, /removed/);
});

test('no cookie is a calm, non-error status', () => {
  const s = describeResult({ ok: false, error: 'no_cookie' });
  assert.equal(s.badge, '');
  assert.match(s.text, /log in/i);
});

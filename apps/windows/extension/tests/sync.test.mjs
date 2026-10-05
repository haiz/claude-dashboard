import { test } from 'node:test';
import assert from 'node:assert/strict';
import { getInstallId, detectBrowser, syncOnce } from '../lib/sync.js';

function memStorage() {
  const bag = {};
  return {
    get: async (k) => ({ [k]: bag[k] }),
    set: async (o) => Object.assign(bag, o),
  };
}

test('getInstallId mints once and reuses', async () => {
  const storage = memStorage();
  const first = await getInstallId(storage);
  const second = await getInstallId(storage);
  assert.equal(first, second);
  assert.match(first, /[0-9a-f-]{36}/);
});

test('detectBrowser reads the UA brand', () => {
  assert.equal(detectBrowser('Mozilla/5.0 ... Edg/120.0'), 'edge');
  assert.equal(detectBrowser('Mozilla/5.0 ... Chrome/120 Brave/1.2'), 'brave');
  assert.equal(detectBrowser('Mozilla/5.0 ... Chrome/120.0'), 'chrome');
  assert.equal(detectBrowser('something else'), 'chrome');
});

test('syncOnce sends the sessionKey cookie to the host and returns its reply', async () => {
  const sent = [];
  const reply = await syncOnce({
    userAgent: 'Chrome/120',
    cookies: { get: async () => ({ value: 'sk-ant-sid01-EXT' }) },
    runtime: {
      sendNativeMessage: async (_host, msg) => {
        sent.push(msg);
        return { ok: true, email: 'a@x.com' };
      },
    },
    storage: memStorage(),
  });
  assert.equal(sent.length, 1);
  assert.equal(sent[0].type, 'sessionKey');
  assert.equal(sent[0].sessionKey, 'sk-ant-sid01-EXT');
  assert.equal(sent[0].browser, 'chrome');
  assert.ok(sent[0].installId);
  assert.deepEqual(reply, { ok: true, email: 'a@x.com' });
});

test('syncOnce with no cookie does not call the host', async () => {
  let called = false;
  const reply = await syncOnce({
    userAgent: 'Chrome/120',
    cookies: { get: async () => null },
    runtime: { sendNativeMessage: async () => { called = true; } },
    storage: memStorage(),
  });
  assert.equal(called, false);
  assert.deepEqual(reply, { ok: false, error: 'no_cookie' });
});

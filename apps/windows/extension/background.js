// Service worker: sync on install, whenever the claude.ai sessionKey cookie
// changes, and on a 30-minute alarm. The result drives the toolbar badge and
// title, and is stored for the popup.

import { syncOnce } from './lib/sync.js';
import { describeResult } from './lib/status.js';

const env = () => ({ cookies: chrome.cookies, runtime: chrome.runtime, storage: chrome.storage.local });

async function runSync() {
  let result;
  try {
    result = await syncOnce(env());
  } catch (e) {
    result = e;
  }
  const status = describeResult(result);
  try {
    await chrome.action.setBadgeText({ text: status.badge });
    await chrome.action.setTitle({ title: status.text });
    await chrome.storage.local.set({ lastStatus: status.text, lastAt: Date.now() });
  } catch {
    // Action/storage unavailable (e.g. during teardown): nothing to do.
  }
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create('sync', { periodInMinutes: 30 });
  runSync();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === 'sync') runSync();
});

chrome.cookies.onChanged.addListener((info) => {
  if (info.cookie && info.cookie.name === 'sessionKey' && info.cookie.domain.includes('claude.ai')) {
    runSync();
  }
});

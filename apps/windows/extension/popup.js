// Shows the last sync status and offers a manual re-sync by waking the worker.

async function show() {
  const { lastStatus } = await chrome.storage.local.get('lastStatus');
  document.getElementById('status').textContent = lastStatus || 'Not synced yet.';
}

document.getElementById('sync').addEventListener('click', async () => {
  // The worker owns syncing; a cookies call wakes it and triggers onChanged,
  // but the simplest reliable nudge is to re-read after a short delay.
  await chrome.runtime.sendMessage?.({ type: 'syncNow' }).catch(() => {});
  setTimeout(show, 500);
});

show();

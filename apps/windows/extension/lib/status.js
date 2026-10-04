// Turns a sync result — a thrown error, an {ok:false} reply, or an {ok:true}
// reply — into { badge, text } for the toolbar action.

const HOST_MISSING = /native messaging host not found|not found|Specified native/i;

export function describeResult(result) {
  if (result instanceof Error) {
    const text = HOST_MISSING.test(result.message)
      ? 'Claude Dashboard is not installed, or its browser host is not registered.'
      : `Sync failed: ${result.message}`;
    return { badge: '!', text };
  }
  if (result && result.ok) {
    return { badge: '', text: `Synced ${result.email || 'account'}` };
  }
  const reply = result || {};
  switch (reply.error) {
    case 'no_cookie':
      return { badge: '', text: 'Log in to claude.ai to sync.' };
    case 'muted':
      return { badge: '!', text: reply.message || 'This account was removed.' };
    default:
      return { badge: '!', text: reply.message || `Sync failed (${reply.error || 'unknown'}).` };
  }
}

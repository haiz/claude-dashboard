// Reads the claude.ai `sessionKey` cookie and forwards it to the local
// native-messaging host. Pure of globals: every browser API comes in through
// `env`, so these functions run under `node --test` with simple mocks.

const HOST = 'com.claude_dashboard.bridge';

// Mints a stable per-install id on first use and reuses it thereafter.
export async function getInstallId(storage) {
  const got = await storage.get('installId');
  if (got && got.installId) {
    return got.installId;
  }
  const installId = crypto.randomUUID();
  await storage.set({ installId });
  return installId;
}

// The browser brand from a user-agent string. Order matters: Edge and Brave
// both also contain "Chrome".
export function detectBrowser(ua) {
  if (/\bEdg\//.test(ua)) return 'edge';
  if (/\bBrave\b/.test(ua)) return 'brave';
  if (/\bChrome\//.test(ua)) return 'chrome';
  return 'chrome';
}

// Reads the cookie and, if present, sends it to the host. Returns the host's
// reply, or `{ ok:false, error:'no_cookie' }` when there is no session to send.
export async function syncOnce(env) {
  const { cookies, runtime, storage } = env;
  const cookie = await cookies.get({ url: 'https://claude.ai', name: 'sessionKey' });
  if (!cookie || !cookie.value) {
    return { ok: false, error: 'no_cookie' };
  }
  const installId = await getInstallId(storage);
  const browser = detectBrowser(env.userAgent || (globalThis.navigator && navigator.userAgent) || '');
  return runtime.sendNativeMessage(HOST, {
    type: 'sessionKey',
    installId,
    browser,
    sessionKey: cookie.value,
  });
}

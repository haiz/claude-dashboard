// The argument shapes are fixed by contract/helper-cli.md and are the same
// ones cli/claude-dashboard-cli uses at lines 348 and 377. The session key is
// passed as argv and must never reach a log line or an error message.

export function decryptArgv(helperPath) {
    return [helperPath, 'decrypt'];
}

// contract/helper-cli.md, "Linux-only commands": every stored account, with no
// session key. Needed because decrypt's inclusion filter
// (status == active && orgId != nil) hides exactly the expired accounts the UI
// has to show, and its six-field projection carries neither `source` nor
// `chromeProfileName` — the two fields the expired-card guidance branches on.
export function listArgv(helperPath) {
    return [helperPath, 'list'];
}

export function usageArgv(helperPath, orgId, sessionKey) {
    return [helperPath, 'usage', orgId, sessionKey];
}

// contract/helper-cli.md: decrypt's failure paths all write to stderr and
// exit 1, so these are stderr messages, not stdout. Both mean "nothing to
// show", as distinct from a real failure.
const EMPTY_STORE_MESSAGES = [
    'No accounts found',
    'No active accounts with session keys found',
];

export function isNoAccountsMessage(stderr) {
    const trimmed = (stderr ?? '').trim();
    return EMPTY_STORE_MESSAGES.some(m => trimmed.startsWith(m));
}

// The six-field decrypt projection (contract/helper-cli.md's decrypt
// section: "Six fields means six keys, always" — name, email, orgId,
// sessionKey, plan, status) carries no identity of its own. Every downstream
// map key — poller.js's usageByAccountId and _lastGood, lib/model.js's
// buildRows, indicator.js's _rowWidgets, accountRow.js's burn-tracker key —
// needs one stable id per account, so it is derived here, once, and attached
// as `id`. That name doesn't claim the helper emits an `id` field; it's
// chosen only so the existing call sites don't need to change.
//
// email is preferred: it's what contract/README.md's "Account identity"
// section itself falls back to once a real accountUuid isn't available.
// orgId is deliberately never used — that same section is explicit that an
// orgId identifies an organisation, not an account, and every member of a
// company org shares one, so keying on it would collapse every colleague
// onto a single row. If both email and name are missing, a fixed sentinel
// keeps the derivation total instead of quietly producing another
// `undefined` that collapses unrelated accounts onto one row.
const UNKNOWN_ACCOUNT_ID = '(unknown account)';

function deriveAccountId(account) {
    const email = typeof account?.email === 'string' ? account.email.trim() : '';
    if (email !== '')
        return email;
    const name = typeof account?.name === 'string' ? account.name.trim() : '';
    if (name !== '')
        return name;
    return UNKNOWN_ACCOUNT_ID;
}

export function parseAccounts(stdout) {
    try {
        const parsed = JSON.parse(stdout);
        if (!Array.isArray(parsed))
            return [];
        return parsed.map(account => ({...account, id: deriveAccountId(account)}));
    } catch {
        return [];
    }
}

// `list` rows carry the store's own unique `id`, but the rest of the extension
// keys on the identity derived from email/name — changing that now would orphan
// every usage-log row, pin and saved command already on disk. So the derived id
// stays the row id, and the store id rides along as `storeId` for `remove`.
export function parseListedAccounts(stdout) {
    let parsed;
    try {
        parsed = JSON.parse(stdout);
    } catch {
        return [];
    }
    if (!Array.isArray(parsed))
        return [];
    return parsed
        .filter(account => account !== null && typeof account === 'object')
        .map(account => ({
            ...account,
            storeId: account.id,
            id: deriveAccountId(account),
        }));
}

// Joins the two commands: `list` decides which rows exist, `decrypt` supplies
// the credentials for the ones that can be polled. An account present only in
// `decrypt` is still kept — the two commands read the same store, so that
// should not happen, but dropping a pollable account because a second process
// rewrote the file mid-refresh would be the worse failure.
export function mergeAccounts(listed, decrypted) {
    const byId = new Map();
    for (const account of listed)
        byId.set(account.id, {...account});
    for (const account of decrypted) {
        const existing = byId.get(account.id);
        if (existing)
            Object.assign(existing, account, {storeId: existing.storeId});
        else
            byId.set(account.id, {...account, storeId: null});
    }
    return [...byId.values()];
}

export function parseUsagePayload(stdout) {
    try {
        return JSON.parse(stdout);
    } catch {
        return null;
    }
}

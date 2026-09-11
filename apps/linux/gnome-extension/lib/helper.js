// The argument shapes are fixed by contract/helper-cli.md and are the same
// ones cli/claude-dashboard-cli uses at lines 348 and 377. The session key is
// passed as argv and must never reach a log line or an error message.

export function decryptArgv(helperPath) {
    return [helperPath, 'decrypt'];
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

export function parseAccounts(stdout) {
    try {
        const parsed = JSON.parse(stdout);
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

export function parseUsagePayload(stdout) {
    try {
        return JSON.parse(stdout);
    } catch {
        return null;
    }
}

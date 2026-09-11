// The argument shapes are fixed by contract/helper-cli.md and are the same
// ones cli/claude-dashboard-cli uses at lines 348 and 377. The session key is
// passed as argv and must never reach a log line or an error message.

export function decryptArgv(helperPath) {
    return [helperPath, 'decrypt'];
}

export function usageArgv(helperPath, orgId, sessionKey) {
    return [helperPath, 'usage', orgId, sessionKey];
}

export function isNoAccountsMessage(stdout) {
    return stdout.trim().startsWith('No accounts found');
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

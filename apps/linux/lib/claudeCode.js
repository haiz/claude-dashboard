// Ports ClaudeCodeAccountDetector
// (apps/macos/ClaudeDashboard/Services/ClaudeCodeAccountDetector.swift): the
// email Claude Code is currently authenticated as, read from ~/.claude.json's
// `oauthAccount.emailAddress`. The macOS detector returns nil when the file is
// missing, malformed, or carries no such field; every one of those paths maps
// to null here.
//
// Reading the file is the caller's job (poller.js) so this stays free of
// gi:// imports and runs under tests/run.js.

export function activeEmailFrom(text) {
    if (typeof text !== 'string' || text === '')
        return null;
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch {
        return null;
    }
    const email = parsed?.oauthAccount?.emailAddress;
    if (typeof email !== 'string')
        return null;
    const trimmed = email.trim();
    return trimmed === '' ? null : trimmed;
}

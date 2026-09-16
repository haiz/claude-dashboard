// Ports UpdateService.swift's decision-making: which release is newer, and
// what the GitHub payload means. The HTTP call and the storage live in
// updateService.js, so this stays free of gi:// imports.
//
// What is NOT ported is the install: macOS downloads ClaudeDashboard.app.zip
// and swaps its own bundle. A GNOME Shell extension cannot safely replace
// either itself or the Rust helper underneath a running session — the helper
// may live in /usr/bin from a .deb, and the extension is loaded into the
// compositor process. So the Linux side checks and tells; installing stays
// with whatever package manager put the files there.

export const REPO_SLUG = 'haiz/claude-dashboard';
export const RELEASES_URL = `https://api.github.com/repos/${REPO_SLUG}/releases/latest`;

// UpdateViewModel's daily auto-check.
export const CHECK_INTERVAL_MS = 24 * 3600 * 1000;

// isNewer(remote:than:) — a component-wise numeric compare, with a missing
// component reading as 0 so "1.18" and "1.18.0" are the same version. A
// non-numeric component is dropped by compactMap on macOS; `parseInt` here
// yields NaN, so it is filtered out the same way.
export function isNewer(remote, current) {
    const parse = v => String(v ?? '').split('.')
        .map(part => parseInt(part, 10))
        .filter(n => Number.isInteger(n));
    const r = parse(remote);
    const c = parse(current);
    const count = Math.max(r.length, c.length);
    for (let i = 0; i < count; i++) {
        const rv = i < r.length ? r[i] : 0;
        const cv = i < c.length ? c[i] : 0;
        if (rv > cv)
            return true;
        if (rv < cv)
            return false;
    }
    return false;
}

// The tag is `vX.Y.Z` on this repo; macOS strips a leading "v" and nothing
// else, so a tag in any other shape is compared verbatim and simply loses.
export function versionFromTag(tagName) {
    const tag = String(tagName ?? '');
    return tag.startsWith('v') ? tag.slice(1) : tag;
}

// Returns {version, url, body} when the payload describes a newer release,
// null when it does not, and throws only on a payload that is not a release
// at all — a caller distinguishes "up to date" from "could not tell".
export function releaseIfNewer(payload, currentVersion) {
    if (payload === null || typeof payload !== 'object' || typeof payload.tag_name !== 'string')
        throw new Error('not a release payload');
    const version = versionFromTag(payload.tag_name);
    if (!isNewer(version, currentVersion))
        return null;
    return {
        version,
        // macOS resolves an asset download URL here; there is nothing to
        // download on Linux, so the release page is what the user is sent to.
        url: typeof payload.html_url === 'string'
            ? payload.html_url
            : `https://github.com/${REPO_SLUG}/releases/tag/${payload.tag_name}`,
        body: typeof payload.body === 'string' ? payload.body : null,
    };
}

// UpdateViewModel.check(respectRateLimit:) refuses to re-check inside the
// interval. `lastCheckMs` of 0 means "never checked", which always passes.
export function shouldCheck({lastCheckMs, nowMs, intervalMs = CHECK_INTERVAL_MS}) {
    if (!Number.isFinite(lastCheckMs) || lastCheckMs <= 0)
        return true;
    return nowMs - lastCheckMs >= intervalMs;
}

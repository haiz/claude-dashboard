// Reader for the daemon's state file. Platform-scoped contract:
// contract/linux-state.md. Pure — no gi:// imports — so the extension, the
// app and the test suite all run the same code.

export const STATE_SCHEMA_VERSION = 1;

// The floor exists so a very short refresh interval cannot make the panel
// flicker into the degraded state on one slow poll.
const STALE_FLOOR_SECONDS = 300;
const DEFAULT_INTERVAL_SECONDS = 120;

function isPlainObject(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

// Returns null for anything a reader must not interpret: malformed JSON, a
// non-object payload, or a schemaVersion this build does not implement.
// Rule 3 of the contract — unknown version means degraded, never guess.
export function parseState(text) {
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch {
        return null;
    }
    if (!isPlainObject(parsed))
        return null;
    if (parsed.schemaVersion !== STATE_SCHEMA_VERSION)
        return null;

    // Defaults keep every consumer free of null checks. Unknown fields on
    // `parsed` survive untouched, per rule 2.
    return {
        ...parsed,
        polledAtMs: typeof parsed.polledAtMs === 'number' ? parsed.polledAtMs : 0,
        daemon: isPlainObject(parsed.daemon) ? parsed.daemon : {},
        accounts: Array.isArray(parsed.accounts) ? parsed.accounts : [],
        usage: isPlainObject(parsed.usage) ? parsed.usage : {},
        errors: isPlainObject(parsed.errors) ? parsed.errors : {},
        fatal: parsed.fatal ?? null,
    };
}

export function isStale(state, nowMs) {
    if (state === null)
        return true;
    const interval = typeof state.daemon.intervalSeconds === 'number'
        ? state.daemon.intervalSeconds
        : DEFAULT_INTERVAL_SECONDS;
    const thresholdMs = Math.max(3 * interval, STALE_FLOOR_SECONDS) * 1000;
    return nowMs - state.polledAtMs > thresholdMs;
}

// Shapes a snapshot into buildRows()' four inputs. The usage payloads are
// passed straight through: decoding them is lib/model.js's job and nobody
// else's.
export function toBuildRowsInput(state) {
    return {
        accounts: state.accounts,
        usageByAccountId: state.usage,
        activeEmail: state.activeClaudeCodeEmail ?? null,
        errorsByAccountId: state.errors,
    };
}

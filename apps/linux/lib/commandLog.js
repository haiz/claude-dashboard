// Ports CommandLogStore.swift and CommandLogModels.swift: the trigger/status
// vocabulary, the newest-first read order, and the 500-entry cap. As with
// lib/usageLog.js, the SQLite file is an implementation detail — this keeps
// rows in memory and the caller persists them as JSON.

// CommandLogModels.swift:5-17. The raw values are persisted, so they are fixed.
export const TRIGGER = {
    manual: 0,      // the user pressed Run
    autoReset: 1,   // a usage window reset was detected
    autoEmpty: 2,   // a refresh produced no 5h/7d usage
};

export const TRIGGER_LABEL = {
    0: 'Manual',
    1: 'Auto (reset)',
    2: 'Auto (empty)',
};

// CommandLogModels.swift:20-32.
export const STATUS = {
    exited: 0,
    timedOut: 1,
    cancelled: 2,
    launchedInTerminal: 3,
    launchFailed: 4,
};

export const STATUS_LABEL = {
    0: 'Exited',
    1: 'Timed out',
    2: 'Cancelled',
    3: 'In Terminal',
    4: 'Launch failed',
};

// CommandLogStore.swift:13's `maxEntries: Int = 500`.
export const MAX_ENTRIES = 500;

// CommandRunner.swift's OutputTail(maxBytes: 4096): only the tail is kept, so a
// chatty command cannot grow the log without bound.
export const MAX_OUTPUT_BYTES = 4096;

export function boundedTail(text, maxBytes = MAX_OUTPUT_BYTES) {
    if (typeof text !== 'string')
        return '';
    // Measured in bytes, not characters, so a multibyte tail obeys the same cap
    // the Swift applies. Slicing from a byte offset can split a code point, so
    // the decoder is asked to be lenient and the replacement char is trimmed.
    const bytes = new TextEncoder().encode(text);
    if (bytes.length <= maxBytes)
        return text;
    const tail = bytes.slice(bytes.length - maxBytes);
    return new TextDecoder('utf-8').decode(tail).replace(/^�+/, '');
}

export class CommandLog {
    constructor(entries = [], maxEntries = MAX_ENTRIES) {
        this._entries = entries;
        this._maxEntries = maxEntries;
        this._nextId = entries.reduce((max, e) => Math.max(max, e.id), 0) + 1;
    }

    get entries() {
        return this._entries;
    }

    record({accountId = null, command, trigger, startedAtMs, finishedAtMs = null,
            status, exitCode = null, output = null}) {
        const entry = {
            id: this._nextId++,
            accountId,
            command,
            trigger,
            startedAtMs,
            finishedAtMs,
            exitCode,
            status,
            output: output === null || output === '' ? null : boundedTail(output),
        };
        this._entries.push(entry);
        this._trim();
        return entry;
    }

    // "DELETE FROM command_logs WHERE id NOT IN (SELECT id ... ORDER BY id DESC
    // LIMIT ?)" — the cap keeps the newest `maxEntries` by id, not by time, so a
    // row recorded out of clock order is still kept in insertion order.
    _trim() {
        if (this._entries.length <= this._maxEntries)
            return;
        this._entries.sort((a, b) => a.id - b.id);
        this._entries = this._entries.slice(this._entries.length - this._maxEntries);
    }

    // CommandLogViewModel reads newest first.
    list() {
        return this._entries.slice().sort((a, b) => b.id - a.id);
    }

    clear() {
        const removed = this._entries.length;
        this._entries = [];
        return removed;
    }
}

const FORMAT_VERSION = 1;

export function serialize(log) {
    return JSON.stringify({version: FORMAT_VERSION, entries: log.entries});
}

export function deserialize(text, maxEntries = MAX_ENTRIES) {
    if (typeof text !== 'string' || text === '')
        return new CommandLog([], maxEntries);
    let parsed;
    try {
        parsed = JSON.parse(text);
    } catch {
        return new CommandLog([], maxEntries);
    }
    if (parsed?.version !== FORMAT_VERSION || !Array.isArray(parsed.entries))
        return new CommandLog([], maxEntries);
    const entries = parsed.entries.filter(e =>
        e !== null && typeof e === 'object' &&
        typeof e.id === 'number' && typeof e.command === 'string' &&
        typeof e.trigger === 'number' && typeof e.status === 'number' &&
        typeof e.startedAtMs === 'number');
    return new CommandLog(entries, maxEntries);
}

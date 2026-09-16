// The gi:// half of the usage log: where it lives on disk, when it is written,
// and the 90-day prune. All the semantics live in lib/usageLog.js, which this
// file only drives.
//
// macOS keeps a SQLite file and writes one row per poll synchronously.
// contract/usage-log.md says the engine is not contract, so this keeps the
// rows in memory and writes a JSON document — but a write per poll would
// rewrite the whole document on every tick, so saves are debounced and the
// pending state is flushed on disable().

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

import {UsageLog, WINDOW, serialize, deserialize} from './lib/usageLog.js';

// DashboardViewModel.swift:131 prunes at 90 days on every launch.
const RETENTION_MS = 90 * 24 * 3600 * 1000;
const SAVE_DEBOUNCE_SECONDS = 20;

function logFile() {
    const dir = GLib.build_filenamev([GLib.get_user_data_dir(), 'claude-dashboard']);
    GLib.mkdir_with_parents(dir, 0o700);
    return Gio.File.new_for_path(GLib.build_filenamev([dir, 'usage-log.json']));
}

export class UsageLogStore {
    constructor() {
        this._file = logFile();
        this._log = this._load();
        this._saveTimeoutId = 0;
        this._dirty = false;

        if (this._log.deleteOlderThan(Date.now() - RETENTION_MS) > 0)
            this._markDirty();
    }

    get log() {
        return this._log;
    }

    _load() {
        try {
            const [ok, bytes] = GLib.file_get_contents(this._file.get_path());
            if (!ok)
                return new UsageLog();
            return deserialize(new TextDecoder().decode(bytes));
        } catch {
            // A missing file is the first-run path, not an error; a corrupt one
            // is regenerable history and is better discarded than repaired.
            return new UsageLog();
        }
    }

    // Records one poll's worth of rows: one per window the account reports.
    // Mirrors BurnRateTracker.swift:36, which logs from the same place the
    // burn rate is computed, so the two never disagree about what was seen.
    recordRows(rows, nowMs = Date.now()) {
        let recorded = 0;
        for (const row of rows) {
            if (!row.windows)
                continue;
            const pairs = [
                [WINDOW.fiveHour, row.windows.fiveHour],
                [WINDOW.sevenDay, row.windows.sevenDay],
                [WINDOW.fable, row.windows.fable],
            ];
            for (const [window, usage] of pairs) {
                // A window with no reset time has never started a cycle; macOS
                // logs `resetsAt` unconditionally because its model carries a
                // non-optional Date, but a null here would encode as epoch 0
                // and invent a cycle boundary.
                if (!usage || usage.resetsAtMs === null)
                    continue;
                this._log.record({
                    accountId: row.id,
                    window,
                    resetsAtMs: usage.resetsAtMs,
                    utilization: usage.utilization,
                    recordedAtMs: nowMs,
                });
                recorded += 1;
            }
        }
        if (recorded > 0)
            this._markDirty();
        return recorded;
    }

    _markDirty() {
        this._dirty = true;
        if (this._saveTimeoutId)
            return;
        this._saveTimeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, SAVE_DEBOUNCE_SECONDS, () => {
            this._saveTimeoutId = 0;
            this.flush();
            return GLib.SOURCE_REMOVE;
        });
    }

    flush() {
        if (!this._dirty)
            return;
        this._dirty = false;
        try {
            this._file.replace_contents(
                new TextEncoder().encode(serialize(this._log)),
                null, false, Gio.FileCreateFlags.REPLACE_DESTINATION, null);
        } catch (e) {
            logError(e, 'claude-dashboard: could not write the usage log');
        }
    }

    destroy() {
        if (this._saveTimeoutId) {
            GLib.Source.remove(this._saveTimeoutId);
            this._saveTimeoutId = 0;
        }
        this.flush();
    }
}

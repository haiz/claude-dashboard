// The gi:// half of the command log, mirroring usageLogStore.js: where it
// lives, when it is written, nothing else. The semantics are in
// lib/commandLog.js.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

import {CommandLog, serialize, deserialize} from './lib/commandLog.js';

const SAVE_DEBOUNCE_SECONDS = 5;

function logFile() {
    const dir = GLib.build_filenamev([GLib.get_user_data_dir(), 'claude-dashboard']);
    GLib.mkdir_with_parents(dir, 0o700);
    return Gio.File.new_for_path(GLib.build_filenamev([dir, 'command-log.json']));
}

export class CommandLogStore {
    constructor() {
        this._file = logFile();
        this._log = this._load();
        this._saveTimeoutId = 0;
        this._dirty = false;
        this._listeners = new Set();
    }

    get log() {
        return this._log;
    }

    _load() {
        try {
            const [ok, bytes] = GLib.file_get_contents(this._file.get_path());
            if (!ok)
                return new CommandLog();
            return deserialize(new TextDecoder().decode(bytes));
        } catch {
            return new CommandLog();
        }
    }

    // The log window redraws from this rather than polling.
    connect(listener) {
        this._listeners.add(listener);
        return () => this._listeners.delete(listener);
    }

    record(row) {
        const entry = this._log.record(row);
        this._markDirty();
        for (const listener of this._listeners)
            listener();
        return entry;
    }

    clear() {
        const removed = this._log.clear();
        this._markDirty();
        for (const listener of this._listeners)
            listener();
        return removed;
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
            logError(e, 'claude-dashboard: could not write the command log');
        }
    }

    destroy() {
        if (this._saveTimeoutId) {
            GLib.Source.remove(this._saveTimeoutId);
            this._saveTimeoutId = 0;
        }
        this._listeners.clear();
        this.flush();
    }
}

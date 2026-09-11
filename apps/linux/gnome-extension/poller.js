import GObject from 'gi://GObject';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {decryptArgv, usageArgv, parseAccounts, parseUsagePayload, isNoAccountsMessage} from './lib/helper.js';
import {buildRows} from './lib/model.js';

const DEFAULT_HELPER_NAMES = ['claude-dashboard-helper'];

function runAsync(argv, cancellable) {
    return new Promise((resolve, reject) => {
        let proc;
        try {
            proc = Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        } catch (e) {
            reject(e);
            return;
        }

        // Cancelling communicate_utf8_async cancels the call, not the child.
        // Without this the helper keeps running orphaned after stop().
        let cancelId = 0;
        if (cancellable)
            cancelId = cancellable.connect(() => proc.force_exit());

        proc.communicate_utf8_async(null, cancellable, (source, res) => {
            try {
                const [, stdout, stderr] = source.communicate_utf8_finish(res);
                resolve({
                    ok: source.get_successful(),
                    stdout: stdout ?? '',
                    stderr: stderr ?? '',
                });
            } catch (e) {
                reject(e);
            } finally {
                if (cancelId > 0)
                    cancellable.disconnect(cancelId);
            }
        });
    });
}

export const Poller = GObject.registerClass({
    Signals: {
        'updated': {param_types: [GObject.TYPE_JSOBJECT]},
        'failed': {param_types: [GObject.TYPE_STRING]},
    },
}, class Poller extends GObject.Object {
    _init(settings) {
        super._init();
        this._settings = settings;
        this._timeoutId = 0;
        this._settingsChangedId = 0;
        this._cancellable = null;
        // One failed fetch must not blank a row that was fine a minute ago, so
        // the last good payload per account id is kept and reused.
        this._lastGood = new Map();
    }

    helperPath() {
        const configured = this._settings?.get_string('helper-path') ?? '';
        if (configured !== '')
            return configured;
        for (const name of DEFAULT_HELPER_NAMES) {
            const found = GLib.find_program_in_path(name);
            if (found)
                return found;
        }
        // The README's Linux one-liner installs the helper to ~/.local/bin,
        // which a Wayland/systemd user session does not reliably carry on
        // PATH. Probe it directly before giving up — this is the same path
        // the `helper-path` setting exists to override.
        const fallback = GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'claude-dashboard-helper']);
        if (GLib.file_test(fallback, GLib.FileTest.EXISTS | GLib.FileTest.IS_EXECUTABLE))
            return fallback;
        return '';
    }

    start() {
        this.refreshNow();
        // _scheduleTimeout() guards against orphaning a previous timeout
        // source, so a second start() without an intervening stop() is safe.
        this._scheduleTimeout();

        // Changing `refresh-interval` in prefs must take effect immediately,
        // not only after the extension is disabled and re-enabled — on
        // Wayland the user cannot restart the Shell to find out otherwise.
        if (!this._settingsChangedId && this._settings) {
            this._settingsChangedId = this._settings.connect('changed::refresh-interval', () => {
                this._scheduleTimeout();
            });
        }
    }

    _scheduleTimeout() {
        if (this._timeoutId) {
            GLib.Source.remove(this._timeoutId);
            this._timeoutId = 0;
        }
        const interval = this._settings?.get_int('refresh-interval') ?? 120;
        this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, interval, () => {
            this.refreshNow();
            return GLib.SOURCE_CONTINUE;
        });
    }

    stop() {
        if (this._timeoutId) {
            GLib.Source.remove(this._timeoutId);
            this._timeoutId = 0;
        }
        if (this._settingsChangedId) {
            this._settings?.disconnect(this._settingsChangedId);
            this._settingsChangedId = 0;
        }
        this._cancel();
    }

    _cancel() {
        if (this._cancellable) {
            this._cancellable.cancel();
            this._cancellable = null;
        }
    }

    refreshNow() {
        // A refresh that arrives mid-flight replaces the previous generation
        // rather than queueing behind it.
        this._cancel();
        this._cancellable = new Gio.Cancellable();
        this._refresh(this._cancellable).catch(e => {
            if (!e.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                this.emit('failed', e.message ?? String(e));
        });
    }

    async _refresh(cancellable) {
        const helper = this.helperPath();
        if (helper === '') {
            this.emit('failed', 'helper-missing');
            return;
        }

        const decrypted = await runAsync(decryptArgv(helper), cancellable);
        if (cancellable.is_cancelled())
            return;

        if (isNoAccountsMessage(decrypted.stderr)) {
            this.emit('updated', []);
            return;
        }
        if (!decrypted.ok) {
            // A real decrypt failure must not masquerade as an empty store.
            this.emit('failed', 'decrypt-failed');
            return;
        }

        const accounts = parseAccounts(decrypted.stdout);
        if (accounts.length === 0) {
            this.emit('updated', []);
            return;
        }

        // A single account's fetch throwing (Gio.Subprocess.new failing, or
        // communicate_utf8_finish rejecting) must not fail Promise.all and
        // take every other row down with it — the design's error table
        // requires a failed `usage` call to leave other rows drawing.
        const settled = await Promise.all(accounts.map(async account => {
            if (!account.orgId || !account.sessionKey)
                return [account.id, null];
            try {
                const res = await runAsync(usageArgv(helper, account.orgId, account.sessionKey), cancellable);
                return [account.id, res.ok ? parseUsagePayload(res.stdout) : null];
            } catch {
                return [account.id, null];
            }
        }));

        if (cancellable.is_cancelled())
            return;

        const live = new Set(accounts.map(a => a.id));
        for (const id of [...this._lastGood.keys()]) {
            if (!live.has(id))
                this._lastGood.delete(id);
        }

        const usageByAccountId = {};
        for (const [id, payload] of settled) {
            if (payload) {
                this._lastGood.set(id, payload);
                usageByAccountId[id] = payload;
            } else {
                const stale = this._lastGood.get(id);
                if (stale)
                    usageByAccountId[id] = stale;
            }
        }

        this.emit('updated', buildRows(accounts, usageByAccountId, Date.now()));
    }
});

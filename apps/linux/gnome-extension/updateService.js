// The gi:// half of the update check: the HTTP request, the daily timer, and
// the notification. All the decisions are in lib/update.js.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Soup from 'gi://Soup';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';

import {RELEASES_URL, CHECK_INTERVAL_MS, releaseIfNewer, shouldCheck} from './lib/update.js';

// How long after enable() the first automatic check runs. Immediately would
// put a network request in the middle of Shell startup.
const STARTUP_DELAY_SECONDS = 90;
const RECHECK_INTERVAL_SECONDS = 3600;

export const UpdateService = GObject.registerClass({
    Signals: {
        // (state, detail): state is 'idle' | 'checking' | 'available' |
        // 'up-to-date' | 'failed'; detail is the version or the error text.
        'state-changed': {param_types: [GObject.TYPE_STRING, GObject.TYPE_STRING]},
    },
}, class UpdateService extends GObject.Object {
    _init({settings, currentVersion}) {
        super._init();
        this._settings = settings;
        this._currentVersion = currentVersion;
        this._session = new Soup.Session({user_agent: `claude-dashboard/${currentVersion}`});
        this._timeoutId = 0;
        this._latestVersion = null;
        this._state = 'idle';
    }

    get currentVersion() {
        return this._currentVersion;
    }

    get latestVersion() {
        return this._latestVersion;
    }

    get state() {
        return this._state;
    }

    start() {
        if (this._timeoutId)
            return;
        this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, STARTUP_DELAY_SECONDS, () => {
            this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, RECHECK_INTERVAL_SECONDS, () => {
                this.checkIfDue();
                return GLib.SOURCE_CONTINUE;
            });
            this.checkIfDue();
            return GLib.SOURCE_REMOVE;
        });
    }

    stop() {
        if (this._timeoutId) {
            GLib.Source.remove(this._timeoutId);
            this._timeoutId = 0;
        }
        this._session?.abort();
    }

    // The daily automatic path: respects both the user's toggle and the
    // once-a-day rate limit.
    checkIfDue() {
        if (!this._settings.get_boolean('auto-update-check'))
            return;
        const lastCheckMs = this._settings.get_double('last-update-check') * 1000;
        if (!shouldCheck({lastCheckMs, nowMs: Date.now(), intervalMs: CHECK_INTERVAL_MS}))
            return;
        this.check({notify: true});
    }

    // The manual path: no rate limit, because the user just asked.
    check({notify = false} = {}) {
        this._setState('checking', '');
        const message = Soup.Message.new('GET', RELEASES_URL);
        message.request_headers.append('Accept', 'application/vnd.github+json');
        message.request_headers.append('X-GitHub-Api-Version', '2022-11-28');

        this._session.send_and_read_async(message, GLib.PRIORITY_DEFAULT, null, (session, res) => {
            // The timestamp is written whatever the outcome: a failing check
            // that retried every hour would be worse than a missed update.
            this._settings.set_double('last-update-check', Date.now() / 1000);
            try {
                const bytes = session.send_and_read_finish(res);
                if (message.get_status() !== Soup.Status.OK) {
                    this._setState('failed', `HTTP ${message.get_status()}`);
                    return;
                }
                const payload = JSON.parse(new TextDecoder().decode(bytes.get_data()));
                const release = releaseIfNewer(payload, this._currentVersion);
                if (release === null) {
                    this._latestVersion = this._currentVersion;
                    this._setState('up-to-date', this._currentVersion);
                    return;
                }
                this._latestVersion = release.version;
                this._setState('available', release.version);
                if (notify)
                    this._notify(release);
            } catch (e) {
                this._setState('failed', e.message ?? String(e));
            }
        });
    }

    _setState(state, detail) {
        this._state = state;
        this.emit('state-changed', state, detail);
    }

    // macOS installs the update itself and shows a progress banner. Here the
    // notification is the whole feature: it says a version exists and opens
    // the release page, because nothing in a Shell extension may overwrite the
    // helper binary or the extension directory under a running session.
    _notify(release) {
        const source = new MessageTray.Source({
            title: 'Claude Dashboard',
            iconName: 'software-update-available-symbolic',
        });
        Main.messageTray.add(source);

        const notification = new MessageTray.Notification({
            source,
            title: `Claude Dashboard ${release.version} is available`,
            body: `You are on ${this._currentVersion}. Update through the same package you installed.`,
        });
        notification.addAction('Release notes', () => {
            Gio.AppInfo.launch_default_for_uri(release.url, null);
        });
        source.addNotification(notification);
    }
});

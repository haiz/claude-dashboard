import GLib from 'gi://GLib';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {ClaudeIndicator} from './indicator.js';
import {Poller} from './poller.js';

export default class ClaudeDashboardExtension extends Extension {
    enable() {
        this._settings = this.getSettings();

        this._indicator = new ClaudeIndicator({
            onRefresh: () => this._poller.refreshNow(),
            onSync: () => this._runSync(),
            onPrefs: () => this.openPreferences(),
            // There is no process to quit — the indicator lives inside the
            // Shell — so the honest analogue of the macOS Quit item is to
            // disable the extension.
            onQuit: () => Main.extensionManager.disableExtension(this.uuid),
        });
        Main.panel.addToStatusArea(this.uuid, this._indicator);

        this._poller = new Poller(this._settings);
        this._updatedId = this._poller.connect('updated', (_p, rows) => this._indicator.setRows(rows));
        this._failedId = this._poller.connect('failed', (_p, reason) => {
            // helper-missing and decrypt-failed are different problems: one
            // means there is nothing to run sync against, the other means the
            // helper ran but could not read the account store, so pointing
            // the user at sync would send them down the wrong path.
            let message;
            switch (reason) {
            case 'helper-missing':
                message = 'claude-dashboard-helper not found on PATH.';
                break;
            case 'decrypt-failed':
                message = 'Could not read the account store.';
                break;
            default:
                message = 'Could not reach the helper.';
            }
            this._indicator.setMessage(message);
        });
        this._poller.start();
    }

    disable() {
        this._poller?.disconnect(this._updatedId);
        this._poller?.disconnect(this._failedId);
        this._poller?.stop();
        this._poller = null;

        this._indicator?.destroy();
        this._indicator = null;
        this._settings = null;
    }

    _runSync() {
        const helper = this._poller.helperPath();
        if (helper === '')
            return;
        // sync scans browsers and can prompt the keyring, so it runs detached
        // and the poller picks up whatever it wrote on the next refresh.
        GLib.spawn_async(null, [helper, 'sync'], null, GLib.SpawnFlags.DEFAULT, null);
    }
}

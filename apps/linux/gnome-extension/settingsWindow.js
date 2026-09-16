// Ports SettingsView.swift: the account list with per-account delete, Add
// Account, Re-sync All, and the auto-refresh controls.
//
// macOS reads and writes AccountStore in-process. The extension has no such
// store, so the account rows come from `claude-dashboard-helper list` and
// deletion goes through `remove` (contract/helper-cli.md, "Linux-only
// commands"). The refresh settings live in this extension's own GSettings
// rather than UserDefaults.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

const WIDTH = 520;
const LIST_MAX_HEIGHT = 260;

// SettingsView.swift's Stepper("Every \(n) min", in: 1...60).
const MIN_MINUTES = 1;
const MAX_MINUTES = 60;

export const SettingsWindow = GObject.registerClass(
class SettingsWindow extends ModalDialog.ModalDialog {
    _init({settings, versionName, listAccounts, removeAccount, onSync, onRefresh, updateService}) {
        super._init({styleClass: 'claude-settings-window', destroyOnClose: false});

        this._settings = settings;
        this._listAccounts = listAccounts;
        this._removeAccount = removeAccount;
        this._onSync = onSync;
        this._onRefresh = onRefresh;
        this._updateService = updateService ?? null;
        // Deleting an account is destructive and irreversible, so the row's
        // trash button arms first and commits on a second press — the same
        // two-step the Command Log's Clear All uses.
        this._confirmingId = null;

        const root = new St.BoxLayout({vertical: true, x_expand: true});
        root.set_style(`width: ${WIDTH}px;`);

        const header = new St.BoxLayout({style_class: 'claude-window-toolbar'});
        header.add_child(new St.Label({
            text: 'Settings',
            style_class: 'claude-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        root.add_child(header);
        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        root.add_child(this._sectionLabel('Accounts'));
        this._listScroll = new St.ScrollView({
            style_class: 'claude-settings-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
        });
        this._listScroll.set_style(`max-height: ${LIST_MAX_HEIGHT}px;`);
        this._list = new St.BoxLayout({vertical: true, style_class: 'claude-settings-list'});
        this._listScroll.set_child(this._list);
        root.add_child(this._listScroll);

        root.add_child(this._sectionLabel('Auto Refresh'));
        const refreshRow = new St.BoxLayout({style_class: 'claude-settings-row'});
        this._autoRefreshToggle = new St.Button({
            label: 'Enabled',
            style_class: 'claude-pill-button',
            toggle_mode: true,
            can_focus: true,
        });
        this._autoRefreshToggle.connect('clicked', () => {
            this._settings.set_boolean('auto-refresh-enabled', this._autoRefreshToggle.checked);
            this._syncRefreshControls();
        });
        refreshRow.add_child(this._autoRefreshToggle);
        refreshRow.add_child(new St.Widget({x_expand: true}));

        this._minusButton = this._stepperButton('−', -1);
        this._intervalLabel = new St.Label({
            style_class: 'claude-settings-value',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._plusButton = this._stepperButton('+', 1);
        refreshRow.add_child(this._minusButton);
        refreshRow.add_child(this._intervalLabel);
        refreshRow.add_child(this._plusButton);
        root.add_child(refreshRow);

        root.add_child(this._sectionLabel('Updates'));
        const updateRow = new St.BoxLayout({style_class: 'claude-settings-row'});
        this._autoUpdateToggle = new St.Button({
            label: 'Check daily',
            style_class: 'claude-pill-button',
            toggle_mode: true,
            can_focus: true,
        });
        this._autoUpdateToggle.connect('clicked', () => {
            this._settings.set_boolean('auto-update-check', this._autoUpdateToggle.checked);
        });
        updateRow.add_child(this._autoUpdateToggle);
        updateRow.add_child(new St.Widget({x_expand: true}));
        this._updateStatus = new St.Label({
            style_class: 'claude-settings-account-meta',
            y_align: Clutter.ActorAlign.CENTER,
        });
        updateRow.add_child(this._updateStatus);
        const checkNow = new St.Button({
            label: 'Check for Updates',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        checkNow.connect('clicked', () => this._updateService?.check({notify: true}));
        updateRow.add_child(checkNow);
        root.add_child(updateRow);

        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        const actions = new St.BoxLayout({style_class: 'claude-settings-row'});
        const add = new St.Button({
            label: 'Add Account',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        add.connect('clicked', () => {
            // SetupView's browser-profile scan is what `sync` already does.
            this._onSync();
            this._status.text = 'Scanning browser profiles… accounts appear after the next refresh.';
        });
        actions.add_child(add);
        actions.add_child(new St.Widget({x_expand: true}));
        const resyncAll = new St.Button({
            label: 'Re-sync All',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        resyncAll.connect('clicked', () => {
            this._onSync();
            this._status.text = 'Re-syncing…';
        });
        actions.add_child(resyncAll);
        root.add_child(actions);

        this._status = new St.Label({style_class: 'claude-settings-status'});
        root.add_child(this._status);

        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));
        root.add_child(new St.Label({
            text: `Claude Dashboard v${versionName}`,
            style_class: 'claude-help-footer',
            x_align: Clutter.ActorAlign.CENTER,
        }));

        this.contentLayout.add_child(root);

        this.setButtons([{
            label: 'Done',
            action: () => this.close(global.get_current_time()),
            key: Clutter.KEY_Escape,
            default: true,
        }]);
    }

    _sectionLabel(text) {
        return new St.Label({text, style_class: 'claude-settings-section'});
    }

    _stepperButton(label, delta) {
        const button = new St.Button({
            label,
            style_class: 'claude-stepper-button',
            can_focus: true,
        });
        button.connect('clicked', () => {
            const minutes = this._intervalMinutes() + delta;
            if (minutes < MIN_MINUTES || minutes > MAX_MINUTES)
                return;
            // The schema stores seconds; SettingsView's stepper is in minutes.
            this._settings.set_int('refresh-interval', minutes * 60);
            this._syncRefreshControls();
        });
        return button;
    }

    _intervalMinutes() {
        return Math.max(MIN_MINUTES, Math.round(this._settings.get_int('refresh-interval') / 60));
    }

    _syncRefreshControls() {
        const enabled = this._settings.get_boolean('auto-refresh-enabled');
        this._autoRefreshToggle.checked = enabled;
        this._autoRefreshToggle.label = enabled ? 'Enabled' : 'Disabled';
        const minutes = this._intervalMinutes();
        this._intervalLabel.text = `Every ${minutes} min`;
        // macOS hides the stepper entirely when auto refresh is off; dimming
        // keeps the layout from jumping, which matters more in a fixed-size
        // Shell dialog than in a resizable window.
        for (const button of [this._minusButton, this._plusButton, this._intervalLabel])
            button.opacity = enabled ? 255 : 100;
        this._minusButton.reactive = enabled && minutes > MIN_MINUTES;
        this._plusButton.reactive = enabled && minutes < MAX_MINUTES;
    }

    // NOT named show() — see CommandLogWindow.present().
    present() {
        this._confirmingId = null;
        this._status.text = '';
        this.open(global.get_current_time());
        this._syncRefreshControls();
        this._syncUpdateControls();
        this.reload();
    }

    // Called when a check finishes while the dialog is open.
    refreshUpdateStatus() {
        this._syncUpdateControls();
    }

    _syncUpdateControls() {
        this._autoUpdateToggle.checked = this._settings.get_boolean('auto-update-check');
        const latest = this._updateService?.latestVersion ?? null;
        const state = this._updateService?.state ?? 'idle';
        if (state === 'checking')
            this._updateStatus.text = 'Checking…';
        else if (state === 'available')
            this._updateStatus.text = `v${latest} available`;
        else if (state === 'up-to-date')
            this._updateStatus.text = 'Up to date';
        else if (state === 'failed')
            this._updateStatus.text = 'Check failed';
        else
            this._updateStatus.text = '';
    }

    reload() {
        this._list.remove_all_children();
        const accounts = this._listAccounts();
        if (accounts.length === 0) {
            this._list.add_child(new St.Label({
                text: 'No accounts yet — press Add Account.',
                style_class: 'claude-settings-empty',
            }));
            return;
        }

        for (const account of accounts) {
            const row = new St.BoxLayout({style_class: 'claude-settings-account'});

            const identity = new St.BoxLayout({vertical: true, y_align: Clutter.ActorAlign.CENTER});
            identity.add_child(new St.Label({
                text: account.email ?? account.name,
                style_class: 'claude-settings-account-name',
            }));
            const detail = [
                account.plan,
                account.status === 'active' ? '' : account.status,
                account.chromeProfileName ?? (account.source === 'manual' ? 'pasted key' : ''),
                account.browser,
            ].filter(part => part && part !== '').join('  ·  ');
            identity.add_child(new St.Label({text: detail, style_class: 'claude-settings-account-meta'}));
            row.add_child(identity);
            row.add_child(new St.Widget({x_expand: true}));

            const arming = this._confirmingId === account.id;
            const remove = new St.Button({
                label: arming ? 'Confirm' : 'Remove',
                style_class: arming
                    ? 'claude-toolbar-button claude-destructive claude-arming'
                    : 'claude-toolbar-button claude-destructive',
                can_focus: true,
                y_align: Clutter.ActorAlign.CENTER,
            });
            remove.connect('clicked', () => {
                if (this._confirmingId !== account.id) {
                    this._confirmingId = account.id;
                    this.reload();
                    return;
                }
                this._confirmingId = null;
                const ok = this._removeAccount(account.id);
                this._status.text = ok
                    ? `Removed ${account.email ?? account.name}.`
                    : `Could not remove ${account.email ?? account.name}.`;
                this.reload();
                this._onRefresh();
            });
            row.add_child(remove);

            this._list.add_child(row);
        }
    }
});

// Runs `claude-dashboard-helper list` and parses it. Returns [] on every
// failure path: an unreadable store is "no accounts to manage", and the
// dialog's own empty state says so.
export function listAccounts(helperPath) {
    if (!helperPath)
        return [];
    try {
        const [, stdout] = GLib.spawn_sync(
            null, [helperPath, 'list'], null, GLib.SpawnFlags.DEFAULT, null);
        const parsed = JSON.parse(new TextDecoder().decode(stdout));
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

export function removeAccount(helperPath, id) {
    if (!helperPath)
        return false;
    try {
        const [, , , status] = GLib.spawn_sync(
            null, [helperPath, 'remove', id], null, GLib.SpawnFlags.DEFAULT, null);
        return status === 0;
    } catch {
        return false;
    }
}

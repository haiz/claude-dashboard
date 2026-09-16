import GLib from 'gi://GLib';

import {WINDOW} from './lib/usageLog.js';

// accountRow.js names its columns with the helper's window keys; the log names
// them with UsageWindow raw values. This is the one place they meet.
const WINDOW_BY_KEY = {
    five_hour: WINDOW.fiveHour,
    seven_day: WINDOW.sevenDay,
    fable: WINDOW.fable,
};

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {ClaudeIndicator} from './indicator.js';
import {DashboardWindow} from './dashboardWindow.js';
import {HelpWindow} from './helpWindow.js';
import {ChartWindow} from './chartWindow.js';
import {CommandLogWindow} from './commandLogWindow.js';
import {CommandLogStore} from './commandLogStore.js';
import {CommandRunner} from './commandRunner.js';
import {RunCommandDialog, savedCommandFor, pruneSavedCommands} from './runCommandDialog.js';
import {SettingsWindow, listAccounts, removeAccount} from './settingsWindow.js';
import {UpdateService} from './updateService.js';
import {UsageLogStore} from './usageLogStore.js';
import {AutoRunLatch} from './lib/autoRun.js';
import {TRIGGER} from './lib/commandLog.js';
import {Poller} from './poller.js';

export default class ClaudeDashboardExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._logStore = new UsageLogStore();
        this._commandLogStore = new CommandLogStore();
        this._commandRunner = new CommandRunner({store: this._commandLogStore});
        this._autoRunLatch = new AutoRunLatch();

        this._indicator = new ClaudeIndicator({
            onRefresh: () => this._poller.refreshNow(),
            onSync: () => this._runSync(),
            onPrefs: () => {
                this._indicator.menu.close();
                this._settingsWindow.present();
            },
            // MenuBarPopover's expand button closes the popover and raises the
            // dashboard window; the Shell's modal grab requires the menu to be
            // down first, so the close is explicit here.
            onExpand: () => {
                this._indicator.menu.close();
                this._dashboardWindow.toggle();
            },
            onHelp: () => {
                this._indicator.menu.close();
                this._helpWindow.toggle();
            },
            onCommandLog: () => {
                this._indicator.menu.close();
                this._commandLogWindow.present();
            },
            onRunCommand: accountId => {
                this._indicator.menu.close();
                this._openRunCommand(accountId);
            },
            onResync: () => {
                this._indicator.menu.close();
                this._runSync();
            },
            onOverview: () => {
                this._indicator.menu.close();
                this._chartWindow.showOverview();
            },
            onOpenChart: (accountId, window) => {
                this._indicator.menu.close();
                this._openChart(accountId, window);
            },
            // Ports DashboardViewModel.togglePin: pinning is exclusive, and
            // re-pinning the account that is already pinned clears the pin.
            onTogglePin: accountId => {
                if (!accountId)
                    return;
                const current = this._settings.get_string('pinned-account');
                this._settings.set_string('pinned-account', current === accountId ? '' : accountId);
                this._poller.refreshNow();
            },
            // There is no process to quit — the indicator lives inside the
            // Shell — so the honest analogue of the macOS Quit item is to
            // disable the extension. PopupBaseMenuItem.activate() emits
            // 'activate' and then calls this._getTopMenu().itemActivated(…)
            // afterwards, so disabling synchronously here would destroy the
            // indicator, menu and item mid-emission. Defer it to idle so the
            // emission finishes first.
            onQuit: () => GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                Main.extensionManager.disableExtension(this.uuid);
                return GLib.SOURCE_REMOVE;
            }),
        });
        Main.panel.addToStatusArea(this.uuid, this._indicator);

        // One BurnRateTracker feeds both surfaces: two trackers would each see
        // half the samples and project different animals for the same window.
        this._dashboardWindow = new DashboardWindow({
            onRefresh: () => this._poller.refreshNow(),
            onPrefs: () => this._settingsWindow.present(),
            onOpenChart: (accountId, window) => this._openChart(accountId, window),
            onRunCommand: accountId => this._openRunCommand(accountId),
            onCommandLog: () => this._commandLogWindow.present(),
            tracker: this._indicator.tracker,
        });

        this._chartWindow = new ChartWindow({store: this._logStore});
        this._commandLogWindow = new CommandLogWindow({store: this._commandLogStore});
        this._updateService = new UpdateService({
            settings: this._settings,
            currentVersion: this.metadata['version-name'] ?? '0.0.0',
        });
        // Repaints the Settings dialog's status line if it is open when a
        // check lands.
        this._updateStateId = this._updateService.connect('state-changed', () => {
            if (this._settingsWindow?.state === 0)
                this._settingsWindow.refreshUpdateStatus();
        });
        this._updateService.start();

        this._settingsWindow = new SettingsWindow({
            settings: this._settings,
            versionName: this.metadata['version-name'] ?? 'dev',
            updateService: this._updateService,
            listAccounts: () => listAccounts(this._poller.helperPath()),
            removeAccount: id => removeAccount(this._poller.helperPath(), id),
            onSync: () => this._runSync(),
            onRefresh: () => this._poller.refreshNow(),
        });
        this._runCommandDialog = new RunCommandDialog({
            settings: this._settings,
            runner: this._commandRunner,
            onRefresh: () => this._poller.refreshNow(),
        });

        // HelpView's footer prints the app version; metadata.json's
        // version-name is the copy scripts/sync-version.sh keeps current.
        this._helpWindow = new HelpWindow({
            versionName: this.metadata['version-name'] ?? 'dev',
        });

        this._poller = new Poller(this._settings);
        this._updatedId = this._poller.connect('updated', (_p, rows) => {
            // Logged before the views render, so a chart opened from this very
            // refresh already sees the sample it is drawing.
            this._logStore.recordRows(rows);
            this._indicator.setRows(rows);
            this._dashboardWindow.setRows(rows);
            this._chartWindow.setRows(rows);
            this._lastRows = rows;
            // RunCommandSettings.prune: commands belonging to accounts that are
            // no longer in the store are unreachable, so they go with them.
            pruneSavedCommands(this._settings, new Set(rows.map(row => row.id)));
            this._runDueCommands(rows);
        });
        this._refreshingId = this._poller.connect('refreshing', (_p, isRefreshing) => {
            this._indicator.setRefreshing(isRefreshing);
            this._dashboardWindow.setRefreshing(isRefreshing);
        });
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
            this._dashboardWindow.setMessage(message);
        });
        this._poller.start();
    }

    disable() {
        this._poller?.disconnect(this._updatedId);
        this._poller?.disconnect(this._refreshingId);
        this._poller?.disconnect(this._failedId);
        this._poller?.stop();
        this._poller = null;

        // destroyOnClose is off, so an open dialog has to be dismissed before
        // it is destroyed or its modal grab outlives the extension.
        this._dashboardWindow?.close(global.get_current_time());
        this._dashboardWindow?.destroy();
        this._dashboardWindow = null;

        this._helpWindow?.close(global.get_current_time());
        this._helpWindow?.destroy();
        this._helpWindow = null;

        this._chartWindow?.close(global.get_current_time());
        this._chartWindow?.destroy();
        this._chartWindow = null;

        this._updateService?.disconnect(this._updateStateId);
        this._updateService?.stop();
        this._updateService = null;

        this._settingsWindow?.close(global.get_current_time());
        this._settingsWindow?.destroy();
        this._settingsWindow = null;

        this._commandLogWindow?.close(global.get_current_time());
        this._commandLogWindow?.destroy();
        this._commandLogWindow = null;

        this._runCommandDialog?.close(global.get_current_time());
        this._runCommandDialog?.destroy();
        this._runCommandDialog = null;

        // applicationWillTerminate's terminateAll: a disabled extension must
        // not leave `claude` or its children running.
        this._commandRunner?.terminateAll();
        this._commandRunner = null;
        this._autoRunLatch = null;

        // Flushes whatever the debounce still owes before the extension goes.
        this._logStore?.destroy();
        this._logStore = null;
        this._commandLogStore?.destroy();
        this._commandLogStore = null;

        this._indicator?.destroy();
        this._indicator = null;
        this._settings = null;
    }

    // A null accountId means the Overview; otherwise the account's detail
    // chart, preselected to the window whose gauge was clicked.
    _openChart(accountId, window) {
        if (accountId === null) {
            this._chartWindow.showOverview();
            return;
        }
        const row = this._chartWindow ? this._lastRows?.find(r => r.id === accountId) : null;
        this._chartWindow.showAccount(accountId, row?.email ?? row?.name ?? accountId,
            WINDOW_BY_KEY[window] ?? WINDOW_BY_KEY.five_hour);
    }

    _openRunCommand(accountId) {
        const row = this._lastRows?.find(r => r.id === accountId);
        this._runCommandDialog.present(accountId, row?.email ?? row?.name ?? accountId);
    }

    // The reset monitor. macOS polls every five seconds; here the poller's own
    // cadence is the clock, because the rows it emits are the only thing the
    // rule reads and a tick between refreshes would see identical data.
    _runDueCommands(rows) {
        for (const accountId of this._autoRunLatch.due(rows)) {
            const command = savedCommandFor(this._settings, accountId);
            if (command === '')
                continue;
            this._commandRunner.run({
                command,
                accountId,
                trigger: TRIGGER.autoReset,
            });
        }
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

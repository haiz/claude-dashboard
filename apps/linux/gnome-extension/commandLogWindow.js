// Ports CommandLogView.swift: the newest-first list of recorded runs, a
// refresh, and Clear All behind a confirmation.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

import {TRIGGER_LABEL, STATUS_LABEL, STATUS} from './lib/commandLog.js';

// CommandLogView.swift:32's .frame(minWidth: 640, minHeight: 400).
const WIDTH = 640;
const MAX_HEIGHT = 400;

function formatTimestamp(ms) {
    const d = new Date(ms);
    const pad = n => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
        `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function formatDuration(startedAtMs, finishedAtMs) {
    if (finishedAtMs === null || finishedAtMs === undefined)
        return '';
    const seconds = Math.max(0, (finishedAtMs - startedAtMs) / 1000);
    return seconds < 10 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`;
}

export const CommandLogWindow = GObject.registerClass(
class CommandLogWindow extends ModalDialog.ModalDialog {
    _init({store}) {
        super._init({styleClass: 'claude-log-window', destroyOnClose: false});

        this._store = store;
        this._confirmingClear = false;

        const root = new St.BoxLayout({vertical: true, x_expand: true});
        root.set_style(`width: ${WIDTH}px;`);

        const header = new St.BoxLayout({style_class: 'claude-window-toolbar'});
        header.add_child(new St.Label({
            text: 'Command Log',
            style_class: 'claude-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        header.add_child(new St.Widget({x_expand: true}));

        const refresh = new St.Button({
            label: 'Refresh',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        refresh.connect('clicked', () => this.reload());
        header.add_child(refresh);

        this._clearButton = new St.Button({
            label: 'Clear All',
            style_class: 'claude-toolbar-button claude-destructive',
            can_focus: true,
        });
        this._clearButton.connect('clicked', () => this._onClear());
        header.add_child(this._clearButton);

        root.add_child(header);
        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        this._scroll = new St.ScrollView({
            style_class: 'claude-log-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
        });
        this._scroll.set_style(`max-height: ${MAX_HEIGHT}px;`);
        this._list = new St.BoxLayout({vertical: true, style_class: 'claude-log-list'});
        this._scroll.set_child(this._list);
        root.add_child(this._scroll);

        this._empty = new St.Label({
            text: 'No commands have run yet.',
            style_class: 'claude-window-empty',
        });
        root.add_child(this._empty);

        this.contentLayout.add_child(root);

        this.setButtons([{
            label: 'Close',
            action: () => this.close(global.get_current_time()),
            key: Clutter.KEY_Escape,
            default: true,
        }]);
    }

    // Clear All is destructive, so the first press arms and the second commits
    // — the Shell has no confirmationDialog for a button inside a dialog, and
    // this is the same two-step macOS gets from its .confirmationDialog.
    _onClear() {
        if (!this._confirmingClear) {
            this._confirmingClear = true;
            this._clearButton.label = 'Confirm Clear';
            return;
        }
        this._confirmingClear = false;
        this._clearButton.label = 'Clear All';
        this._store.clear();
        this.reload();
    }

    // NOT named show(): ModalDialog._fadeOpen() calls this.show() to make the
    // actor visible, so a subclass method of that name shadows
    // Clutter.Actor.show() and the dialog opens without ever appearing —
    // state reaches OPENED while visible stays false, with no error anywhere.
    present() {
        this._confirmingClear = false;
        this._clearButton.label = 'Clear All';
        this.open(global.get_current_time());
        this.reload();
    }

    reload() {
        this._list.remove_all_children();
        const entries = this._store.log.list();
        this._empty.visible = entries.length === 0;
        this._scroll.visible = entries.length > 0;

        for (const entry of entries) {
            const row = new St.BoxLayout({vertical: true, style_class: 'claude-log-row'});

            const top = new St.BoxLayout({style_class: 'claude-log-row-top'});
            top.add_child(new St.Label({
                text: entry.command,
                style_class: 'claude-log-command',
                y_align: Clutter.ActorAlign.CENTER,
            }));
            top.add_child(new St.Widget({x_expand: true}));

            const failed = entry.status === STATUS.launchFailed ||
                entry.status === STATUS.timedOut ||
                (entry.status === STATUS.exited && entry.exitCode !== 0 && entry.exitCode !== null);
            const code = entry.exitCode === null || entry.exitCode === undefined
                ? '' : ` ${entry.exitCode}`;
            top.add_child(new St.Label({
                text: `${STATUS_LABEL[entry.status] ?? ''}${code}`,
                style_class: failed ? 'claude-log-status claude-log-failed' : 'claude-log-status',
                y_align: Clutter.ActorAlign.CENTER,
            }));
            row.add_child(top);

            const meta = [
                formatTimestamp(entry.startedAtMs),
                TRIGGER_LABEL[entry.trigger] ?? '',
                formatDuration(entry.startedAtMs, entry.finishedAtMs),
                entry.accountId ?? '',
            ].filter(part => part !== '').join('   ·   ');
            row.add_child(new St.Label({text: meta, style_class: 'claude-log-meta'}));

            if (entry.output) {
                const output = new St.Label({
                    text: entry.output.trimEnd(),
                    style_class: 'claude-log-output',
                });
                output.clutter_text.line_wrap = true;
                row.add_child(output);
            }

            this._list.add_child(row);
            this._list.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));
        }
    }
});

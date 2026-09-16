// Ports DashboardWindow.swift's `.dashboard` case: the toolbar, and the
// adaptive grid of non-compact AccountCards.
//
// macOS opens this as a real NSWindow. A GNOME Shell extension lives inside
// the compositor and has no window of its own to open, so the nearest
// equivalent is a Shell modal dialog — it dims the session, takes the keyboard
// grab, closes on Escape, and can host the very same St actors the popover
// uses. That keeps one AccountRow implementation driving both size classes
// rather than a second, drifting copy.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

import {AccountRow} from './accountRow.js';

// LazyVGrid(columns: [GridItem(.adaptive(minimum: 420, maximum: 480), spacing: 12)])
// (DashboardWindow.swift:105). The column count is derived the same way:
// as many 420-wide columns as fit, at least one.
const CARD_MIN_WIDTH = 420;
const CARD_MAX_WIDTH = 480;
const GRID_SPACING = 12;

// DashboardWindow.swift:38's .frame(minWidth: 1050, minHeight: 450), clamped
// to the monitor so a small screen still gets a usable dialog.
const PREFERRED_WIDTH = 1050;
const MONITOR_MARGIN = 120;

function toolbarButton(iconName, label, callback) {
    const box = new St.BoxLayout({style_class: 'claude-toolbar-button-content'});
    box.add_child(new St.Icon({
        icon_name: iconName,
        icon_size: 16,
        y_align: Clutter.ActorAlign.CENTER,
    }));
    box.add_child(new St.Label({text: label, y_align: Clutter.ActorAlign.CENTER}));

    const button = new St.Button({
        child: box,
        style_class: 'claude-toolbar-button',
        can_focus: true,
    });
    button.connect('clicked', () => callback());
    return button;
}

export const DashboardWindow = GObject.registerClass(
class DashboardWindow extends ModalDialog.ModalDialog {
    _init({onRefresh, onPrefs, onOpenChart, onRunCommand, onCommandLog, tracker}) {
        // Reopening must not need a fresh instance: the extension keeps one
        // and toggles it, so destroyOnClose has to be off.
        super._init({styleClass: 'claude-dashboard-window', destroyOnClose: false});

        this._tracker = tracker;
        this._onOpenChart = onOpenChart ?? null;
        this._onRunCommand = onRunCommand ?? null;
        this._rowWidgets = new Map();
        this._cards = new Map();
        this._rows = [];
        this._isRefreshing = false;
        this._columnCount = 0;

        const root = new St.BoxLayout({vertical: true, x_expand: true, y_expand: true});

        const toolbar = new St.BoxLayout({style_class: 'claude-window-toolbar'});
        toolbar.add_child(new St.Label({
            text: 'Claude Dashboard',
            style_class: 'claude-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        toolbar.add_child(new St.Widget({x_expand: true}));
        toolbar.add_child(toolbarButton('org.gnome.Settings-privacy-diagnostics-symbolic', 'Overview', () => {
            this.close(global.get_current_time());
            this._onOpenChart?.(null, null);
        }));
        this._refreshButton = toolbarButton('view-refresh-symbolic', 'Refresh', () => onRefresh());
        toolbar.add_child(this._refreshButton);
        toolbar.add_child(toolbarButton('view-list-bullet-symbolic', 'Command Log', () => {
            this.close(global.get_current_time());
            onCommandLog?.();
        }));
        toolbar.add_child(toolbarButton('preferences-system-symbolic', 'Settings', () => onPrefs()));
        root.add_child(toolbar);

        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        this._scroll = new St.ScrollView({
            style_class: 'claude-window-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
            y_expand: true,
        });
        // The grid is a column of rows rather than a flow layout: Clutter's
        // flow layouts size children to the widest one, which would stretch a
        // two-gauge card to a three-gauge card's width. macOS caps the column
        // at 480 instead, so the width is imposed per card here.
        this._grid = new St.BoxLayout({vertical: true, style_class: 'claude-window-grid', x_expand: true});
        this._empty = new St.Label({style_class: 'claude-window-empty'});
        this._grid.add_child(this._empty);
        this._scroll.set_child(this._grid);
        root.add_child(this._scroll);

        this.contentLayout.add_child(root);

        // Escape closes, matching a window's Cmd-W / red X.
        this.setButtons([{
            label: 'Close',
            action: () => this.close(global.get_current_time()),
            key: Clutter.KEY_Escape,
            default: true,
        }]);

        this.connect('opened', () => this._applyMonitorSize());
    }

    _applyMonitorSize() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const width = Math.min(PREFERRED_WIDTH, monitor.width - MONITOR_MARGIN);
        this.contentLayout.set_style(`width: ${width}px;`);
        this._scroll.set_style(`max-height: ${Math.max(450, monitor.height - 260)}px;`);

        // As many CARD_MIN_WIDTH columns as fit, at least one — the port of
        // GridItem(.adaptive(minimum: 420, …)).
        const columns = Math.max(1, Math.floor((width + GRID_SPACING) / (CARD_MIN_WIDTH + GRID_SPACING)));
        if (columns !== this._columnCount) {
            this._columnCount = columns;
            this._relayout();
        }
    }

    setRefreshing(isRefreshing) {
        if (this._isRefreshing === isRefreshing)
            return;
        this._isRefreshing = isRefreshing;
        this._refreshButton.reactive = !isRefreshing;
        const now = Date.now();
        for (const row of this._rows)
            this._rowWidgets.get(row.id)?.update(row, this._tracker, now, isRefreshing);
    }

    setMessage(text) {
        this._rows = [];
        this._discardCards();
        this._empty.text = text;
        this._empty.show();
    }

    setRows(rows) {
        this._rows = rows;
        if (rows.length === 0) {
            // DashboardWindow.swift:126-149's empty state. The "Add Account"
            // button it carries needs an account-management surface the Linux
            // helper does not expose yet, so the copy stands alone for now.
            this.setMessage('No Accounts\nSync your Claude accounts from your browser to get started.');
            return;
        }
        this._empty.hide();

        const now = Date.now();
        const seen = new Set();
        for (const row of rows) {
            seen.add(row.id);
            let widget = this._rowWidgets.get(row.id);
            if (!widget) {
                widget = new AccountRow({
                    isCompact: false,
                    onRunCommand: id => {
                        this.close(global.get_current_time());
                        this._onRunCommand?.(id);
                    },
                    onOpenChart: (id, window) => {
                        // The chart is its own modal; this one has to let go of
                        // the grab before that one takes it.
                        this.close(global.get_current_time());
                        this._onOpenChart?.(id, window);
                    },
                });
                this._rowWidgets.set(row.id, widget);
                const card = new St.Bin({style_class: 'claude-card', child: widget});
                card.set_style(`max-width: ${CARD_MAX_WIDTH}px;`);
                this._cards.set(row.id, card);
            }
            widget.update(row, this._tracker, now, this._isRefreshing);
        }

        for (const id of [...this._rowWidgets.keys()]) {
            if (!seen.has(id)) {
                this._cards.get(id)?.destroy();
                this._cards.delete(id);
                this._rowWidgets.delete(id);
            }
        }

        this._relayout();
    }

    // Rebuilds the row-of-rows that stands in for the grid. Cards are removed
    // from their old parent first so re-running this after a reorder or a
    // column-count change moves them rather than duplicating them.
    _relayout() {
        const columns = Math.max(1, this._columnCount);
        for (const card of this._cards.values())
            card.get_parent()?.remove_child(card);
        this._grid.remove_all_children();
        this._grid.add_child(this._empty);
        this._empty.visible = this._rows.length === 0;

        let gridRow = null;
        this._rows.forEach((row, index) => {
            const card = this._cards.get(row.id);
            if (!card)
                return;
            if (index % columns === 0) {
                gridRow = new St.BoxLayout({style_class: 'claude-grid-row', x_expand: true});
                this._grid.add_child(gridRow);
            }
            gridRow.add_child(card);
        });
    }

    _discardCards() {
        for (const card of this._cards.values())
            card.destroy();
        this._cards.clear();
        this._rowWidgets.clear();
        this._grid.remove_all_children();
        this._grid.add_child(this._empty);
    }

    toggle() {
        if (this.state === ModalDialog.State.OPENED || this.state === ModalDialog.State.OPENING)
            this.close(global.get_current_time());
        else
            this.open(global.get_current_time());
    }
});

import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {panelRing} from './ring.js';
import {AccountRow} from './accountRow.js';
import {BurnRateTracker} from './lib/burnRate.js';

export const ClaudeIndicator = GObject.registerClass(
class ClaudeIndicator extends PanelMenu.Button {
    _init({onRefresh, onSync, onPrefs, onQuit, onTogglePin, onExpand, onHelp, onOverview,
           onOpenChart, onCommandLog, onRunCommand, onResync}) {
        super._init(0.5, 'Claude Dashboard');

        this._tracker = new BurnRateTracker();
        // Shared with the dashboard window so both read one sample history.
        this.tracker = this._tracker;
        this._rowWidgets = new Map();
        this._onTogglePin = onTogglePin ?? null;
        this._onOpenChart = onOpenChart ?? null;
        this._onRunCommand = onRunCommand ?? null;
        this._onResync = onResync ?? null;
        this._isRefreshing = false;
        this._rows = [];

        const box = new St.BoxLayout({style_class: 'claude-panel-box'});
        this._panelRing = panelRing();
        this._panelLabel = new St.Label({
            text: '--%',
            style_class: 'claude-panel-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(this._panelRing);
        box.add_child(this._panelLabel);
        this.add_child(box);

        const header = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        const headerBox = new St.BoxLayout({x_expand: true});
        headerBox.add_child(new St.Label({
            text: 'Claude Dashboard',
            style_class: 'claude-account-name',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        headerBox.add_child(new St.Widget({x_expand: true}));
        // MenuBarPopover.swift's header row. 'expand' opens the dashboard
        // window, the same as the rectangle.expand.vertical button there.
        for (const [icon, cb] of [['view-refresh-symbolic', onRefresh],
                                  ['emblem-synchronizing-symbolic', onSync],
                                  ['view-fullscreen-symbolic', onExpand],
                                  ['org.gnome.Settings-privacy-diagnostics-symbolic', onOverview],
                                  ['view-list-bullet-symbolic', onCommandLog],
                                  ['help-about-symbolic', onHelp],
                                  ['preferences-system-symbolic', onPrefs]]) {
            const button = new St.Button({
                child: new St.Icon({icon_name: icon, icon_size: 16}),
                style_class: 'icon-button',
            });
            button.connect('clicked', () => cb?.());
            headerBox.add_child(button);
        }
        header.add_child(headerBox);
        this.menu.addMenuItem(header);

        // macOS wraps the cards in a ScrollView capped at 400pt
        // (MenuBarPopover.swift:124) so a long account list scrolls instead of
        // growing the popover past the screen. The cap itself lives in
        // stylesheet.css as max-height on .claude-popup-scroll.
        this._body = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        this._scroll = new St.ScrollView({
            style_class: 'claude-popup-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
        });
        this._bodyBox = new St.BoxLayout({vertical: true, style_class: 'claude-popup-content', x_expand: true});
        this._empty = new St.Label({style_class: 'claude-status-text'});
        this._bodyBox.add_child(this._empty);
        this._scroll.set_child(this._bodyBox);
        this._body.add_child(this._scroll);
        this.menu.addMenuItem(this._body);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        const quit = new PopupMenu.PopupMenuItem('Turn Off Claude Dashboard');
        quit.connect('activate', () => onQuit());
        this.menu.addMenuItem(quit);

        this.setMessage('Loading…');
    }

    // macOS's per-card ProgressView is driven by isRefreshing; the rows are
    // re-rendered rather than reaching into each widget so a row created
    // mid-pass still picks the flag up.
    setRefreshing(isRefreshing) {
        if (this._isRefreshing === isRefreshing)
            return;
        this._isRefreshing = isRefreshing;
        const now = Date.now();
        for (const row of this._rows) {
            const widget = this._rowWidgets.get(row.id);
            widget?.update(row, this._tracker, now, isRefreshing);
        }
    }

    setMessage(text) {
        for (const widget of this._rowWidgets.values())
            widget.destroy();
        this._rowWidgets.clear();
        this._rows = [];
        this._empty.show();
        this._empty.text = text;
        this._panelLabel.text = '--%';
        this._panelRing.setValue(0);
    }

    setRows(rows) {
        if (rows.length === 0) {
            this.setMessage('No accounts yet — run claude-dashboard-cli sync.');
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
                    onTogglePin: id => this._onTogglePin?.(id),
                    onOpenChart: (id, window) => this._onOpenChart?.(id, window),
                    onRunCommand: id => this._onRunCommand?.(id),
                    onResync: () => this._onResync?.(),
                });
                this._rowWidgets.set(row.id, widget);
            } else {
                this._bodyBox.remove_child(widget);
            }
            this._bodyBox.add_child(widget);
            widget.update(row, this._tracker, now, this._isRefreshing);
        }

        for (const [id, widget] of [...this._rowWidgets]) {
            if (!seen.has(id)) {
                widget.destroy();
                this._rowWidgets.delete(id);
            }
        }

        this._rows = rows;

        // macOS's menu bar reads one number: the top-sorted account's 5-hour
        // utilization (DashboardViewModel.menuBarSource, which falls back to
        // `accountStates.first { $0.usage != nil }?.usage?.fiveHour`). rows is
        // already burn-rate sorted, so the first row with usage is that
        // account — and because tier 1 of the sort is "pinned first", the
        // pinned account is that first row, which is exactly menuBarSource's
        // own `accountStates.first { $0.account.isPinned }` preference.
        // Taking a max across every account and both windows instead made the
        // panel read 90% off a 7d window while the 5h ring read 3%.
        const source = rows.find(row => row.windows) ?? null;
        if (source === null) {
            this._panelRing.setValue(0);
            this._panelLabel.text = '--%';
            return;
        }
        this._panelRing.setValue(source.windows.fiveHour.utilization);
        this._panelLabel.text = `${Math.round(source.windows.fiveHour.utilization)}%`;
    }
});

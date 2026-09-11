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
    _init({onRefresh, onSync, onPrefs, onQuit}) {
        super._init(0.5, 'Claude Dashboard');

        this._tracker = new BurnRateTracker();
        this._rowWidgets = new Map();

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
        for (const [icon, cb] of [['view-refresh-symbolic', onRefresh],
                                  ['emblem-synchronizing-symbolic', onSync],
                                  ['preferences-system-symbolic', onPrefs]]) {
            const button = new St.Button({
                child: new St.Icon({icon_name: icon, icon_size: 16}),
                style_class: 'icon-button',
            });
            button.connect('clicked', () => cb());
            headerBox.add_child(button);
        }
        header.add_child(headerBox);
        this.menu.addMenuItem(header);

        this._body = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        this._bodyBox = new St.BoxLayout({vertical: true, style_class: 'claude-popup-content', x_expand: true});
        this._empty = new St.Label({style_class: 'claude-status-text'});
        this._bodyBox.add_child(this._empty);
        this._body.add_child(this._bodyBox);
        this.menu.addMenuItem(this._body);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        const quit = new PopupMenu.PopupMenuItem('Turn Off Claude Dashboard');
        quit.connect('activate', () => onQuit());
        this.menu.addMenuItem(quit);

        this.setMessage('Loading…');
    }

    setMessage(text) {
        for (const widget of this._rowWidgets.values())
            widget.destroy();
        this._rowWidgets.clear();
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
        let highest = 0;

        for (const row of rows) {
            seen.add(row.id);
            let widget = this._rowWidgets.get(row.id);
            if (!widget) {
                widget = new AccountRow();
                this._rowWidgets.set(row.id, widget);
            } else {
                this._bodyBox.remove_child(widget);
            }
            this._bodyBox.add_child(widget);
            widget.update(row, this._tracker, now);

            if (row.windows) {
                highest = Math.max(highest,
                    row.windows.fiveHour.utilization,
                    row.windows.sevenDay.utilization);
            }
        }

        for (const [id, widget] of [...this._rowWidgets]) {
            if (!seen.has(id)) {
                widget.destroy();
                this._rowWidgets.delete(id);
            }
        }

        this._panelRing.setValue(highest);
        this._panelLabel.text = `${Math.round(highest)}%`;
    }
});

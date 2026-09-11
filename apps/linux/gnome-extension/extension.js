import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {panelRing} from './ring.js';

const Indicator = GObject.registerClass(
class Indicator extends PanelMenu.Button {
    _init() {
        super._init(0.5, 'Claude Dashboard');
        const box = new St.BoxLayout({style_class: 'claude-panel-box'});
        this._ring = panelRing();
        this._label = new St.Label({
            text: '--%',
            style_class: 'claude-panel-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(this._ring);
        box.add_child(this._label);
        this.add_child(box);
    }

    setHighest(utilization) {
        this._ring.setValue(utilization);
        this._label.text = `${Math.round(utilization)}%`;
    }
});

export default class ClaudeDashboardExtension extends Extension {
    enable() {
        this._indicator = new Indicator();
        // A hard-coded value proves the Cairo path before the poller exists.
        this._indicator.setHighest(62);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class ClaudeDashboardPrefs extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();

        const page = new Adw.PreferencesPage();
        const group = new Adw.PreferencesGroup({title: 'Refresh'});

        const interval = new Adw.SpinRow({
            title: 'Refresh interval',
            subtitle: 'Seconds between helper calls',
            adjustment: new Gtk.Adjustment({lower: 30, upper: 3600, step_increment: 30}),
        });
        settings.bind('refresh-interval', interval, 'value', 0);
        group.add(interval);

        const helperPath = new Adw.EntryRow({title: 'Helper path (blank to use PATH)'});
        settings.bind('helper-path', helperPath, 'text', 0);
        group.add(helperPath);

        page.add(group);
        window.add(page);
    }
}

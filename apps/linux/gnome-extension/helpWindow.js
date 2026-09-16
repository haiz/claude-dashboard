// Ports HelpView.swift — the same six sections, in the same order, with the
// same headings. The prose is adapted where it named something that does not
// exist on Linux (the macOS Keychain, Gatekeeper's "Open Anyway"); everything
// else is the macOS copy.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

// HelpView.swift's .frame(width: 520, height: 560).
const WIDTH = 520;
const MAX_HEIGHT = 560;

// HelpView's content, as data rather than as a wall of actor construction.
// `kind` picks the renderer: 'body' is bodyText(), 'step' is step(), 'bullet'
// is bullet(), 'tip' is tip(), and 'qa' is problemAnswer().
const SECTIONS = [
    {
        title: 'Getting Started',
        icon: 'starred-symbolic',
        items: [
            {kind: 'body', text: 'Claude Dashboard shows your Claude.ai token usage across multiple accounts, pulled directly from your browser sessions — no passwords required.'},
            {kind: 'step', number: 1, text: 'Sign in to claude.ai in a Chromium browser (one profile per account).'},
            {kind: 'step', number: 2, text: 'Run claude-dashboard-cli sync, or press the sync button in this panel menu.'},
            {kind: 'step', number: 3, text: 'The panel picks the synced accounts up on its next refresh.'},
            {kind: 'tip', text: 'If your login keyring prompts for access, allow it — the helper uses it to decrypt the browser’s cookie store.'},
        ],
    },
    {
        title: 'Reading Your Usage',
        icon: 'view-list-symbolic',
        items: [
            {kind: 'body', text: 'Each account card shows up to three usage gauges:'},
            {kind: 'bullet', text: '5h — tokens used in the rolling 5-hour window.'},
            {kind: 'bullet', text: '7d — tokens used in the rolling 7-day window.'},
            {kind: 'bullet', text: 'F — 7-day usage for the Fable model (shown only when your plan scopes it).'},
            {kind: 'body', text: 'Ring colour signals how much is left: green (plenty), yellow (halfway), red (nearly out). The countdown next to each ring shows when that window resets.'},
            {kind: 'body', text: 'The animal emoji reflects your burn rate — a sloth means you’re using tokens slowly; a cheetah means you’re burning through them fast.'},
            {kind: 'body', text: 'A green dot on an account name means that account is currently active in Claude Code.'},
        ],
    },
    {
        title: 'Working With the Dashboard',
        icon: 'view-grid-symbolic',
        items: [
            {kind: 'body', text: 'The panel menu header has four icon buttons, left to right:'},
            {kind: 'bullet', text: 'Refresh — fetch the latest usage now.'},
            {kind: 'bullet', text: 'Sync — rescan your browser profiles for new or renewed sessions.'},
            {kind: 'bullet', text: 'Expand — open the full dashboard window.'},
            {kind: 'bullet', text: 'Settings — refresh interval and helper path.'},
            {kind: 'body', text: 'Right-click a card to pin it to the top of the list regardless of burn rate. A pinned account is also the one the panel percentage reads from.'},
        ],
    },
    {
        title: 'Managing Accounts',
        icon: 'system-users-symbolic',
        items: [
            {kind: 'bullet', text: 'Add an account — run claude-dashboard-cli sync with the browser profile signed in to claude.ai.'},
            {kind: 'bullet', text: 'Re-sync — run the same sync again; an expired session is picked back up once you log in.'},
            {kind: 'bullet', text: 'Remove an account — edit ~/.config/claude-dashboard/accounts.json.'},
            {kind: 'tip', text: 'If a card shows an orange triangle, the session has expired. Open the matching browser profile, log into claude.ai, then sync.'},
        ],
    },
    {
        title: 'Auto-Refresh & Updates',
        icon: 'view-refresh-symbolic',
        items: [
            {kind: 'body', text: 'Settings sets how often the helper is asked for fresh usage, anywhere from 30 seconds to an hour. The macOS app defaults to two minutes and so does this.'},
            {kind: 'body', text: 'Updates are installed the way you installed the helper — there is no in-app updater on Linux yet.'},
        ],
    },
    {
        title: 'Troubleshooting',
        icon: 'applications-engineering-symbolic',
        items: [
            {kind: 'qa', problem: '"claude-dashboard-helper not found on PATH"', answer: 'Install the helper, or set its full path in Settings. A Wayland session started by systemd does not always carry ~/.local/bin on PATH.'},
            {kind: 'qa', problem: '"Could not read the account store"', answer: 'The helper ran but could not decrypt ~/.config/claude-dashboard/accounts.json. Run claude-dashboard-cli sync to rebuild it.'},
            {kind: 'qa', problem: 'Account stuck on "Session expired"', answer: 'Open the matching browser profile, log into claude.ai, then sync again.'},
            {kind: 'qa', problem: 'Usage not updating', answer: 'Press Refresh in the panel menu. Make sure you have an active claude.ai session in the browser for each tracked account.'},
        ],
    },
];

function bodyLabel(text, styleClass) {
    const label = new St.Label({text, style_class: styleClass});
    label.clutter_text.line_wrap = true;
    label.clutter_text.ellipsize = 0; // Pango.EllipsizeMode.NONE
    return label;
}

function renderItem(item) {
    switch (item.kind) {
    case 'step': {
        const row = new St.BoxLayout({style_class: 'claude-help-row'});
        row.add_child(new St.Label({text: `${item.number}.`, style_class: 'claude-help-marker'}));
        row.add_child(bodyLabel(item.text, 'claude-help-body'));
        return row;
    }
    case 'bullet': {
        const row = new St.BoxLayout({style_class: 'claude-help-row'});
        row.add_child(new St.Label({text: '•', style_class: 'claude-help-marker'}));
        row.add_child(bodyLabel(item.text, 'claude-help-body'));
        return row;
    }
    case 'tip': {
        const row = new St.BoxLayout({style_class: 'claude-help-tip'});
        row.add_child(new St.Icon({
            icon_name: 'dialog-information-symbolic',
            icon_size: 14,
            style_class: 'claude-help-tip-icon',
            y_align: Clutter.ActorAlign.START,
        }));
        row.add_child(bodyLabel(item.text, 'claude-help-secondary'));
        return row;
    }
    case 'qa': {
        const box = new St.BoxLayout({vertical: true, style_class: 'claude-help-qa'});
        box.add_child(bodyLabel(item.problem, 'claude-help-problem'));
        box.add_child(bodyLabel(item.answer, 'claude-help-secondary'));
        return box;
    }
    default:
        return bodyLabel(item.text, 'claude-help-body');
    }
}

export const HelpWindow = GObject.registerClass(
class HelpWindow extends ModalDialog.ModalDialog {
    _init({versionName}) {
        super._init({styleClass: 'claude-help-window', destroyOnClose: false});

        const root = new St.BoxLayout({vertical: true, x_expand: true});
        root.set_style(`width: ${WIDTH}px;`);

        const header = new St.BoxLayout({style_class: 'claude-window-toolbar'});
        header.add_child(new St.Label({
            text: 'Help',
            style_class: 'claude-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        root.add_child(header);
        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        const scroll = new St.ScrollView({
            style_class: 'claude-help-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
        });
        scroll.set_style(`max-height: ${MAX_HEIGHT}px;`);
        const content = new St.BoxLayout({vertical: true, style_class: 'claude-help-content'});

        for (const section of SECTIONS) {
            const group = new St.BoxLayout({vertical: true, style_class: 'claude-help-section'});
            const heading = new St.BoxLayout({style_class: 'claude-help-heading'});
            heading.add_child(new St.Icon({
                icon_name: section.icon,
                icon_size: 16,
                y_align: Clutter.ActorAlign.CENTER,
            }));
            heading.add_child(new St.Label({
                text: section.title,
                style_class: 'claude-help-title',
                y_align: Clutter.ActorAlign.CENTER,
            }));
            group.add_child(heading);
            for (const item of section.items)
                group.add_child(renderItem(item));
            content.add_child(group);
        }

        scroll.set_child(content);
        root.add_child(scroll);

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

    toggle() {
        if (this.state === ModalDialog.State.OPENED || this.state === ModalDialog.State.OPENING)
            this.close(global.get_current_time());
        else
            this.open(global.get_current_time());
    }
});

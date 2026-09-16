// Ports RunCommandSheet.swift: a command field remembered per account, a
// "run in terminal" toggle whose default comes from the classifier but whose
// user setting wins, streamed output, and a refresh once the run finishes.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

import {KIND} from './lib/commandClassifier.js';
import {TRIGGER, STATUS, STATUS_LABEL} from './lib/commandLog.js';

const WIDTH = 460;
const OUTPUT_MAX_HEIGHT = 220;

// macOS keys the saved command by Account.id in UserDefaults
// (RunCommandSettings). The helper's accounts have no UUID, so the extension
// keys by the same derived id every other surface uses, in one GSettings
// string holding a JSON object. Same lifetime, same per-account scoping.
function readMap(settings, key) {
    try {
        const parsed = JSON.parse(settings.get_string(key));
        return parsed !== null && typeof parsed === 'object' ? parsed : {};
    } catch {
        return {};
    }
}

function writeMap(settings, key, map) {
    settings.set_string(key, JSON.stringify(map));
}

export function savedCommandFor(settings, accountId) {
    return readMap(settings, 'run-commands')[accountId] ?? '';
}

export function setSavedCommand(settings, accountId, command) {
    const map = readMap(settings, 'run-commands');
    if (command === '')
        delete map[accountId];
    else
        map[accountId] = command;
    writeMap(settings, 'run-commands', map);
}

export function savedTerminalPreference(settings, accountId) {
    return readMap(settings, 'run-command-terminals')[accountId] === true;
}

export function setSavedTerminalPreference(settings, accountId, inTerminal) {
    const map = readMap(settings, 'run-command-terminals');
    if (inTerminal)
        map[accountId] = true;
    else
        delete map[accountId];
    writeMap(settings, 'run-command-terminals', map);
}

// Drops the saved command and toggle of accounts that no longer exist — the
// port of RunCommandSettings.prune(keeping:in:).
export function pruneSavedCommands(settings, liveIds) {
    for (const key of ['run-commands', 'run-command-terminals']) {
        const map = readMap(settings, key);
        let changed = false;
        for (const id of Object.keys(map)) {
            if (!liveIds.has(id)) {
                delete map[id];
                changed = true;
            }
        }
        if (changed)
            writeMap(settings, key, map);
    }
}

export const RunCommandDialog = GObject.registerClass(
class RunCommandDialog extends ModalDialog.ModalDialog {
    _init({settings, runner, onRefresh}) {
        super._init({styleClass: 'claude-run-window', destroyOnClose: false});

        this._settings = settings;
        this._runner = runner;
        this._onRefresh = onRefresh ?? (() => {});
        this._accountId = null;
        this._isRunning = false;
        // Set once the user touches the toggle: from then on the classifier no
        // longer overrides it for this command (RunCommandSheet's
        // userTouchedToggle).
        this._userTouchedToggle = false;

        const root = new St.BoxLayout({vertical: true, style_class: 'claude-run-content'});
        root.set_style(`width: ${WIDTH}px;`);

        this._title = new St.Label({text: 'Run Command', style_class: 'claude-window-title'});
        root.add_child(this._title);

        this._entry = new St.Entry({
            hint_text: 'Enter command…',
            style_class: 'claude-run-entry',
            can_focus: true,
            x_expand: true,
        });
        this._entry.clutter_text.connect('activate', () => this._run());
        this._entry.clutter_text.connect('text-changed', () => {
            // A new command re-arms the classifier's default.
            this._userTouchedToggle = false;
            this._reclassify();
        });
        root.add_child(this._entry);

        const toggleRow = new St.BoxLayout({style_class: 'claude-run-toggle-row'});
        this._terminalToggle = new St.Button({
            label: 'Open in Terminal',
            style_class: 'claude-pill-button',
            toggle_mode: true,
            can_focus: true,
        });
        this._terminalToggle.connect('clicked', () => {
            this._userTouchedToggle = true;
            setSavedTerminalPreference(this._settings, this._accountId, this._terminalToggle.checked);
        });
        toggleRow.add_child(this._terminalToggle);
        toggleRow.add_child(new St.Widget({x_expand: true}));
        this._statusLabel = new St.Label({
            style_class: 'claude-run-status',
            y_align: Clutter.ActorAlign.CENTER,
        });
        toggleRow.add_child(this._statusLabel);
        root.add_child(toggleRow);

        this._outputScroll = new St.ScrollView({
            style_class: 'claude-run-output-scroll',
            hscrollbar_policy: St.PolicyType.AUTOMATIC,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
        });
        this._outputScroll.set_style(`max-height: ${OUTPUT_MAX_HEIGHT}px;`);
        // An St.ScrollView's child must implement StScrollable; a bare
        // St.Label does not, so the label is wrapped in a box that does.
        const outputBox = new St.BoxLayout({vertical: true, x_expand: true});
        this._output = new St.Label({style_class: 'claude-run-output'});
        this._output.clutter_text.line_wrap = true;
        outputBox.add_child(this._output);
        this._outputScroll.set_child(outputBox);
        this._outputScroll.hide();
        root.add_child(this._outputScroll);

        this.contentLayout.add_child(root);

        this.setButtons([
            {
                label: 'Close',
                action: () => this.close(global.get_current_time()),
                key: Clutter.KEY_Escape,
            },
            {
                label: 'Run',
                action: () => this._run(),
                default: true,
            },
        ]);
        this._runButton = this.buttonLayout.get_children().at(-1);
    }

    // NOT named show() — see CommandLogWindow.present() for why.
    present(accountId, accountName) {
        this.open(global.get_current_time());
        this._accountId = accountId;
        this._title.text = `Run Command — ${accountName}`;
        this._entry.set_text(savedCommandFor(this._settings, accountId));
        this._terminalToggle.checked = savedTerminalPreference(this._settings, accountId);
        this._userTouchedToggle = this._terminalToggle.checked;
        this._output.text = '';
        this._outputScroll.hide();
        this._statusLabel.text = '';
        this._entry.grab_key_focus();
        this._reclassify();
    }

    // The classifier only supplies a default; once the user has set the toggle
    // for this command text, it is left alone.
    _reclassify() {
        if (this._userTouchedToggle)
            return;
        const command = this._entry.get_text().trim();
        if (command === '')
            return;
        this._runner.classifyCommand(command).then(kind => {
            if (this._userTouchedToggle || this._entry.get_text().trim() !== command)
                return;
            this._terminalToggle.checked = kind === KIND.interactive;
        }).catch(() => {});
    }

    _run() {
        if (this._isRunning)
            return;
        const command = this._entry.get_text().trim();
        if (command === '')
            return;

        setSavedCommand(this._settings, this._accountId, command);
        setSavedTerminalPreference(this._settings, this._accountId, this._terminalToggle.checked);

        if (this._terminalToggle.checked) {
            const result = this._runner.launchInTerminal({
                command, accountId: this._accountId, trigger: TRIGGER.manual,
            });
            this._finish(result);
            return;
        }

        this._isRunning = true;
        this._runButton.reactive = false;
        this._statusLabel.text = 'Running…';
        this._output.text = '';
        this._outputScroll.show();

        this._runner.run({
            command,
            accountId: this._accountId,
            trigger: TRIGGER.manual,
            onOutput: chunk => {
                this._output.text += chunk;
            },
        }).then(result => {
            this._isRunning = false;
            this._runButton.reactive = true;
            this._finish(result);
        });
    }

    _finish(result) {
        const code = result.exitCode === null || result.exitCode === undefined
            ? '' : ` (exit ${result.exitCode})`;
        this._statusLabel.text = `${STATUS_LABEL[result.status] ?? ''}${code}`;
        if (result.status === STATUS.launchedInTerminal || result.status === STATUS.launchFailed) {
            this._output.text = result.outputTail;
            this._outputScroll.show();
        }
        // macOS refreshes after a run so the usage the command consumed shows up.
        this._onRefresh();
    }
});

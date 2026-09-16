import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import {UsageRing, CountdownRing} from './ring.js';
import {metricsFor, percentFontSize, percentSignFontSize, countdownFontSize} from './lib/geometry.js';
import {formatResetTime, formattedCountdown} from './lib/format.js';
import {planBadgeColor} from './lib/plan.js';

const FIVE_HOUR_SECONDS = 18000;
const SEVEN_DAY_SECONDS = 604800;

const WindowColumn = GObject.registerClass(
class WindowColumn extends St.BoxLayout {
    _init(label, totalSeconds, showCountdown = true, isCompact = true, onTap = null) {
        const metrics = metricsFor(isCompact);
        super._init({vertical: true, x_align: Clutter.ActorAlign.CENTER, reactive: onTap !== null});
        this._onTap = onTap;
        if (onTap) {
            this.connect('button-press-event', (_actor, event) => {
                // Secondary click belongs to the row's pin gesture.
                if (event.get_button() !== Clutter.BUTTON_PRIMARY)
                    return Clutter.EVENT_PROPAGATE;
                this._onTap();
                return Clutter.EVENT_STOP;
            });
        }
        // Ports UsageBar.swift's VStack(spacing: 5) — the label sits above
        // the ring row with a gap, not flush against it. That 5 is fixed in
        // both size classes, unlike the HStack's gap below.
        this.set_style(`spacing: ${metrics.columnGap}px;`);
        this._totalSeconds = totalSeconds;

        const windowLabel = new St.Label({
            text: label,
            style_class: 'claude-window-label',
            x_align: Clutter.ActorAlign.CENTER,
        });
        windowLabel.set_style(`font-size: ${metrics.windowLabelFontSize}px;`);
        this.add_child(windowLabel);

        // Ports UsageBar.swift's HStack(spacing: isCompact ? 5 : 8).
        const gauges = new St.BoxLayout({y_align: Clutter.ActorAlign.END});
        gauges.set_style(`spacing: ${metrics.ringGap}px;`);
        this._ring = new UsageRing(metrics.largeDiameter, metrics.largeLineWidth);

        // macOS sets the number at diameter x 0.33 and the percent sign at
        // diameter x 0.20, baseline-aligned. St has no baseline alignment, so
        // the two labels are bottom-aligned in a row, which reads the same at
        // these sizes. The sizes still come from geometry.js.
        const percentBox = new St.BoxLayout({
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._percentNumber = new St.Label({y_align: Clutter.ActorAlign.END});
        this._percentNumber.set_style(
            `font-size: ${percentFontSize(metrics.largeDiameter)}px; font-weight: 600;`);
        this._percentSign = new St.Label({text: '%', y_align: Clutter.ActorAlign.END});
        this._percentSign.set_style(
            `font-size: ${percentSignFontSize(metrics.largeDiameter)}px;`);
        percentBox.add_child(this._percentNumber);
        percentBox.add_child(this._percentSign);

        // The burn-rate animal sits at the bottom-trailing corner of the ring,
        // as it does on macOS, and is blank until a projection exists.
        this._animal = new St.Label({
            text: '',
            x_align: Clutter.ActorAlign.END,
            y_align: Clutter.ActorAlign.END,
        });
        this._animal.set_style(`font-size: ${metrics.animalFontSize}px;`);

        const ringStack = new St.Widget({
            layout_manager: new Clutter.BinLayout(),
            y_align: Clutter.ActorAlign.END,
        });
        ringStack.add_child(this._ring);
        ringStack.add_child(percentBox);
        ringStack.add_child(this._animal);
        gauges.add_child(ringStack);

        // macOS hides the Fable countdown (AccountCard.swift:128,
        // showCountdown: false) because Fable resets in lockstep with 7d and a
        // second clock would just duplicate it. Not building the column at all
        // is the port of that.
        if (!showCountdown) {
            this._countdownBox = null;
            this.add_child(gauges);
            return;
        }

        this._countdownBox = new St.BoxLayout({
            vertical: true,
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.END,
        });
        this._resetLabel = new St.Label({style_class: 'claude-reset-label'});
        this._countdown = new CountdownRing(metrics.smallDiameter, metrics.smallLineWidth);
        this._countdownText = new St.Label({x_align: Clutter.ActorAlign.CENTER, y_align: Clutter.ActorAlign.CENTER});
        this._countdownText.set_style(
            `font-size: ${countdownFontSize(metrics.smallDiameter)}px; font-family: monospace;`);
        const cdStack = new St.Widget({layout_manager: new Clutter.BinLayout()});
        cdStack.add_child(this._countdown);
        cdStack.add_child(this._countdownText);
        this._countdownBox.add_child(this._resetLabel);
        this._countdownBox.add_child(cdStack);
        gauges.add_child(this._countdownBox);

        this.add_child(gauges);
    }

    update(window, animal, nowMs) {
        this._ring.setValue(window.utilization);
        this._percentNumber.text = `${Math.round(window.utilization)}`;
        this._animal.text = animal ?? '';

        if (this._countdownBox === null)
            return;
        if (window.resetsAtMs === null) {
            this._countdownBox.hide();
            return;
        }
        this._countdownBox.show();
        const remaining = Math.max(0, (window.resetsAtMs - nowMs) / 1000);
        this._countdown.setValue(remaining, this._totalSeconds);
        this._resetLabel.text = formatResetTime(new Date(window.resetsAtMs), this._totalSeconds, new Date(nowMs));
        this._countdownText.text = formattedCountdown(remaining, this._totalSeconds);
    }
});

export const AccountRow = GObject.registerClass(
class AccountRow extends St.BoxLayout {
    _init({onTogglePin, onOpenChart, onRunCommand, onResync, isCompact = true} = {}) {
        super._init({vertical: true, style_class: 'claude-account-row', reactive: true});

        this._onTogglePin = onTogglePin ?? null;
        this._onOpenChart = onOpenChart ?? null;
        this._onRunCommand = onRunCommand ?? null;
        this._onResync = onResync ?? null;
        this._isCompact = isCompact;
        // macOS puts Pin/Unpin behind the card's .contextMenu (AccountCard.swift
        // :100-110). The Shell has no context-menu primitive for a menu item, so
        // the nearest equivalent is the same gesture: secondary click.
        this.connect('button-press-event', (_actor, event) => {
            if (event.get_button() !== Clutter.BUTTON_SECONDARY)
                return Clutter.EVENT_PROPAGATE;
            this._onTogglePin?.(this._accountId);
            return Clutter.EVENT_STOP;
        });

        const header = new St.BoxLayout();

        // macOS stacks name over email (AccountCard.swift:21-36) and drops the
        // email line when it is absent or identical to the name.
        const identity = new St.BoxLayout({vertical: true, y_align: Clutter.ActorAlign.CENTER});
        const nameRow = new St.BoxLayout();
        nameRow.set_style('spacing: 6px;');
        this._name = new St.Label({
            style_class: 'claude-account-name',
            y_align: Clutter.ActorAlign.CENTER,
        });
        // The 8px green dot marking the account Claude Code is signed in as.
        this._activeDot = new St.Widget({
            style_class: 'claude-active-dot',
            width: 8,
            height: 8,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._activeDot.hide();
        nameRow.add_child(this._name);
        nameRow.add_child(this._activeDot);
        this._email = new St.Label({style_class: 'claude-account-email'});
        this._email.hide();
        identity.add_child(nameRow);
        identity.add_child(this._email);
        header.add_child(identity);

        this._pinIcon = new St.Icon({
            icon_name: 'view-pin-symbolic',
            icon_size: 12,
            style_class: 'claude-pin-icon',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._pinIcon.hide();
        header.add_child(this._pinIcon);

        header.add_child(new St.Widget({x_expand: true}));

        // AccountCard.swift:47-62 puts Run Command immediately left of the plan
        // badge. Hidden when no handler is wired, so a surface that cannot run
        // commands does not show a dead button.
        this._runButton = new St.Button({
            child: new St.Icon({icon_name: 'utilities-terminal-symbolic', icon_size: 14}),
            style_class: 'claude-terminal-button',
            can_focus: true,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._runButton.connect('clicked', () => this._onRunCommand?.(this._accountId));
        this._runButton.visible = this._onRunCommand !== null;
        header.add_child(this._runButton);

        this._plan = new St.Label({style_class: 'claude-plan-pill', y_align: Clutter.ActorAlign.CENTER});
        header.add_child(this._plan);

        // macOS shows a spinner while that account's fetch is in flight and an
        // orange warning triangle once it is expired (AccountCard.swift:71-77).
        this._spinner = new St.Icon({
            icon_name: 'content-loading-symbolic',
            icon_size: 12,
            style_class: 'claude-row-spinner',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._spinner.hide();
        header.add_child(this._spinner);
        this._warning = new St.Icon({
            icon_name: 'dialog-warning-symbolic',
            icon_size: 14,
            style_class: 'claude-row-warning',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._warning.hide();
        header.add_child(this._warning);

        this.add_child(header);

        this._gauges = new St.BoxLayout({
            style_class: 'claude-gauges',
            x_align: Clutter.ActorAlign.CENTER,
        });
        // HStack(alignment: .top, spacing: isCompact ? 20 : 30)
        // (AccountCard.swift:121).
        this._gauges.set_style(`spacing: ${isCompact ? 20 : 30}px;`);
        const openChart = this._onOpenChart
            ? key => () => this._onOpenChart(this._accountId, key)
            : () => null;
        this._columns = {
            five_hour: new WindowColumn('5h', FIVE_HOUR_SECONDS, true, isCompact, openChart('five_hour')),
            seven_day: new WindowColumn('7d', SEVEN_DAY_SECONDS, true, isCompact, openChart('seven_day')),
            fable: new WindowColumn('F', SEVEN_DAY_SECONDS, false, isCompact, openChart('fable')),
        };
        for (const key of ['five_hour', 'seven_day', 'fable'])
            this._gauges.add_child(this._columns[key]);
        this.add_child(this._gauges);

        this._status = new St.Label({style_class: 'claude-status-text'});
        this._status.hide();
        this.add_child(this._status);

        // macOS renders a per-account fetch failure as red subheadline text
        // under the card (AccountCard.swift:93-96), separate from the
        // expired/no-usage states above.
        this._error = new St.Label({style_class: 'claude-error-text'});
        this._error.hide();
        this.add_child(this._error);

        // AccountCard.expiredContent (AccountCard.swift:136-160): the guidance
        // differs by where the key came from, and there is a Re-sync button.
        // Reachable only now that `claude-dashboard-helper list` exposes
        // `source` and `chromeProfileName` — `decrypt` carries neither, and
        // filters expired accounts out entirely.
        this._expiredBox = new St.BoxLayout({vertical: true, style_class: 'claude-expired-box'});
        this._expiredHint = new St.Label({style_class: 'claude-status-text'});
        this._expiredHint.clutter_text.line_wrap = true;
        this._expiredBox.add_child(this._expiredHint);
        this._resyncButton = new St.Button({
            label: 'Re-sync',
            style_class: 'claude-toolbar-button',
            can_focus: true,
            x_align: Clutter.ActorAlign.START,
        });
        this._resyncButton.connect('clicked', () => this._onResync?.(this._accountId));
        this._expiredBox.add_child(this._resyncButton);
        this._expiredBox.hide();
        this.add_child(this._expiredBox);
    }

    update(row, tracker, nowMs, isRefreshing = false) {
        this._accountId = row.id;
        this._name.text = row.name;
        if (row.email && row.email !== row.name) {
            this._email.text = row.email;
            this._email.show();
        } else {
            this._email.hide();
        }

        this._activeDot.visible = row.isActiveClaudeCode === true;
        this._pinIcon.visible = row.isPinned === true;

        // The wire value (contract/account-schema.md: "Pro", "Max 5x",
        // "Max 20x", "Max") is already the display string — no mapping needed.
        this._plan.text = row.plan;
        this._plan.set_style(`background-color: ${planBadgeColor(row.plan)};`);

        // The spinner and the warning are mutually exclusive on macOS: the
        // loading branch wins while a fetch is in flight.
        this._spinner.visible = isRefreshing;
        this._warning.visible = !isRefreshing && row.status === 'expired';

        if (row.error) {
            this._error.text = row.error;
            this._error.show();
        } else {
            this._error.hide();
        }

        if (row.status === 'expired') {
            this._gauges.hide();
            this._status.show();
            this._status.text = 'Session expired.';
            this._expiredBox.show();
            // A pasted key has no browser profile to reopen and Re-sync cannot
            // fix it, so it is told where the key actually comes from instead
            // of being pointed at a button that will not help.
            if (row.source === 'manual') {
                this._expiredHint.text =
                    'This key was pasted by hand. Add it again with ' +
                    'claude-dashboard-helper add-key.';
                this._resyncButton.hide();
            } else if (row.chromeProfileName) {
                this._expiredHint.text =
                    `Open browser profile "${row.chromeProfileName}" and log in to ` +
                    'claude.ai, then re-sync.';
                this._resyncButton.show();
            } else {
                this._expiredHint.text =
                    'Log in to claude.ai in your browser, then re-sync.';
                this._resyncButton.show();
            }
            return;
        }
        this._expiredBox.hide();
        if (!row.windows) {
            this._gauges.hide();
            this._status.show();
            this._status.text = 'Could not fetch usage.';
            return;
        }

        this._status.hide();
        this._gauges.show();

        const pairs = [
            ['five_hour', row.windows.fiveHour],
            ['seven_day', row.windows.sevenDay],
            ['fable', row.windows.fable],
        ];
        for (const [key, window] of pairs) {
            const column = this._columns[key];
            if (!window) {
                column.hide();
                continue;
            }
            column.show();
            const projected = tracker.record({
                accountId: row.id,
                window: key,
                utilization: window.utilization,
                resetsAtMs: window.resetsAtMs,
                recordedAtMs: nowMs,
            });
            column.update(window, projected?.animal ?? null, nowMs);
        }
    }
});

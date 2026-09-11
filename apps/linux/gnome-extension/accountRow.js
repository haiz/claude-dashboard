import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import {UsageRing, CountdownRing} from './ring.js';
import {METRICS, percentFontSize, percentSignFontSize, countdownFontSize} from './lib/geometry.js';
import {formatResetTime, formattedCountdown} from './lib/format.js';

const FIVE_HOUR_SECONDS = 18000;
const SEVEN_DAY_SECONDS = 604800;

const WindowColumn = GObject.registerClass(
class WindowColumn extends St.BoxLayout {
    _init(label, totalSeconds) {
        super._init({vertical: true, x_align: Clutter.ActorAlign.CENTER});
        this._totalSeconds = totalSeconds;

        this.add_child(new St.Label({
            text: label,
            style_class: 'claude-window-label',
            x_align: Clutter.ActorAlign.CENTER,
        }));

        const gauges = new St.BoxLayout({y_align: Clutter.ActorAlign.END});
        this._ring = new UsageRing(METRICS.largeDiameter, METRICS.largeLineWidth);

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
            `font-size: ${percentFontSize(METRICS.largeDiameter)}px; font-weight: 600;`);
        this._percentSign = new St.Label({text: '%', y_align: Clutter.ActorAlign.END});
        this._percentSign.set_style(
            `font-size: ${percentSignFontSize(METRICS.largeDiameter)}px;`);
        percentBox.add_child(this._percentNumber);
        percentBox.add_child(this._percentSign);

        // The burn-rate animal sits at the bottom-trailing corner of the ring,
        // as it does on macOS, and is blank until a projection exists.
        this._animal = new St.Label({
            text: '',
            x_align: Clutter.ActorAlign.END,
            y_align: Clutter.ActorAlign.END,
        });
        this._animal.set_style(`font-size: ${METRICS.animalFontSize}px;`);

        const ringStack = new St.Widget({layout_manager: new Clutter.BinLayout()});
        ringStack.add_child(this._ring);
        ringStack.add_child(percentBox);
        ringStack.add_child(this._animal);
        gauges.add_child(ringStack);

        this._countdownBox = new St.BoxLayout({vertical: true, x_align: Clutter.ActorAlign.CENTER});
        this._resetLabel = new St.Label({style_class: 'claude-reset-label'});
        this._countdown = new CountdownRing(METRICS.smallDiameter, METRICS.smallLineWidth);
        this._countdownText = new St.Label({x_align: Clutter.ActorAlign.CENTER, y_align: Clutter.ActorAlign.CENTER});
        this._countdownText.set_style(
            `font-size: ${countdownFontSize(METRICS.smallDiameter)}px; font-family: monospace;`);
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
    _init() {
        super._init({vertical: true, style_class: 'claude-account-row'});

        const header = new St.BoxLayout();
        this._name = new St.Label({
            style_class: 'claude-account-name',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._plan = new St.Label({style_class: 'claude-plan-pill', y_align: Clutter.ActorAlign.CENTER});
        header.add_child(this._name);
        header.add_child(new St.Widget({x_expand: true}));
        header.add_child(this._plan);
        this.add_child(header);

        this._gauges = new St.BoxLayout({style_class: 'claude-gauges'});
        this._columns = {
            five_hour: new WindowColumn('5h', FIVE_HOUR_SECONDS),
            seven_day: new WindowColumn('7d', SEVEN_DAY_SECONDS),
            fable: new WindowColumn('F', SEVEN_DAY_SECONDS),
        };
        for (const key of ['five_hour', 'seven_day', 'fable'])
            this._gauges.add_child(this._columns[key]);
        this.add_child(this._gauges);

        this._status = new St.Label({style_class: 'claude-status-text'});
        this._status.hide();
        this.add_child(this._status);
    }

    update(row, tracker, nowMs) {
        this._name.text = row.email ?? row.name;
        this._plan.text = planLabel(row.plan);

        if (row.status === 'expired') {
            this._gauges.hide();
            this._status.show();
            this._status.text = 'Session expired — re-sync this account.';
            return;
        }
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

function planLabel(plan) {
    switch (plan) {
    case 'pro': return 'Pro';
    case 'max5x': return 'Max 5x';
    case 'max20x': return 'Max 20x';
    case 'max200': return 'Max 200';
    default: return 'Max';
    }
}

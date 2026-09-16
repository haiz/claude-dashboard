// Ports AccountDetailView.swift and OverviewChartView.swift as two modes of
// one modal dialog, since on GNOME they are the same surface with a different
// series set rather than two navigation destinations in one window.
//
//   'detail'   — one account, a 5h / 7d / F window picker, the reset-cycle
//                list, and the measure tool.
//   'overview' — every account on one chart, a legend that toggles accounts,
//                and the optional total line.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

import {ChartArea, SERIES_COLORS} from './chartArea.js';
import {WINDOW, WINDOW_LABEL, withResetTransitions} from './lib/usageLog.js';
import {fullRange, measure, nearestEntry, totalSeries} from './lib/chart.js';
import {formatResetTime} from './lib/format.js';

const CHART_WIDTH = 900;
const CHART_HEIGHT = 320;
// AccountDetailViewModel's initial visibleRange: the last 24 hours.
const DEFAULT_SPAN_MS = 86_400_000;
const BUFFER_SAMPLES = 2;

function pillButton(label, onClick) {
    const button = new St.Button({
        label,
        style_class: 'claude-pill-button',
        toggle_mode: true,
        can_focus: true,
    });
    button.connect('clicked', () => onClick(button));
    return button;
}

function swatch(color) {
    const dot = new St.Widget({width: 10, height: 10, y_align: Clutter.ActorAlign.CENTER});
    dot.set_style(
        `background-color: rgb(${Math.round(color.r * 255)}, ${Math.round(color.g * 255)}, ` +
        `${Math.round(color.b * 255)}); border-radius: 5px;`);
    return dot;
}

export const ChartWindow = GObject.registerClass(
class ChartWindow extends ModalDialog.ModalDialog {
    _init({store}) {
        super._init({styleClass: 'claude-chart-window', destroyOnClose: false});

        this._store = store;
        this._mode = 'overview';
        this._accountId = null;
        this._accountName = '';
        this._window = WINDOW.fiveHour;
        this._rows = [];
        this._hiddenAccounts = new Set();
        this._showTotal = false;
        this._measureActive = false;
        this._measurePoints = [];
        this._selectedCycleIndex = null;
        this._hoverMs = null;

        const root = new St.BoxLayout({vertical: true, x_expand: true});
        root.set_style(`width: ${CHART_WIDTH}px;`);

        // --- Toolbar ---
        const toolbar = new St.BoxLayout({style_class: 'claude-window-toolbar'});
        this._title = new St.Label({
            style_class: 'claude-window-title',
            y_align: Clutter.ActorAlign.CENTER,
        });
        toolbar.add_child(this._title);
        toolbar.add_child(new St.Widget({x_expand: true}));

        // The 5h / 7d / F picker. Present in both modes: the Overview chart
        // has the same picker on macOS (OverviewChartView.selectedWindow).
        this._windowButtons = new Map();
        const picker = new St.BoxLayout({style_class: 'claude-pill-group'});
        for (const key of [WINDOW.fiveHour, WINDOW.sevenDay, WINDOW.fable]) {
            const button = pillButton(WINDOW_LABEL[key], () => this._selectWindow(key));
            this._windowButtons.set(key, button);
            picker.add_child(button);
        }
        toolbar.add_child(picker);

        this._showAllButton = new St.Button({
            label: 'Show All',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        this._showAllButton.connect('clicked', () => this._showAll());
        toolbar.add_child(this._showAllButton);

        this._measureButton = pillButton('Measure', () => this._toggleMeasure());
        toolbar.add_child(this._measureButton);

        this._totalButton = pillButton('Total', () => {
            this._showTotal = !this._showTotal;
            this._totalButton.checked = this._showTotal;
            this._reload();
        });
        toolbar.add_child(this._totalButton);

        this._allAccountsButton = new St.Button({
            label: 'All accounts',
            style_class: 'claude-toolbar-button',
            can_focus: true,
        });
        this._allAccountsButton.connect('clicked', () => this.showOverview());
        toolbar.add_child(this._allAccountsButton);

        root.add_child(toolbar);
        root.add_child(new St.Widget({style_class: 'claude-window-divider', x_expand: true}));

        // --- Chart ---
        this._chart = new ChartArea({height: CHART_HEIGHT, x_expand: true});
        this._chart.connect('hover-changed', (_a, ms) => this._onHover(ms));
        this._chart.connect('range-changed', () => this._reload({keepRange: true}));
        this._chart.connect('point-picked', (_a, ms) => this._onPointPicked(ms));
        root.add_child(this._chart);

        this._readout = new St.Label({style_class: 'claude-chart-readout'});
        root.add_child(this._readout);

        // --- Legend (overview) / cycles (detail) ---
        this._legend = new St.BoxLayout({style_class: 'claude-chart-legend'});
        root.add_child(this._legend);

        this._cyclesBox = new St.BoxLayout({vertical: true, style_class: 'claude-cycles'});
        root.add_child(this._cyclesBox);

        this._empty = new St.Label({style_class: 'claude-window-empty'});
        root.add_child(this._empty);

        this.contentLayout.add_child(root);

        this.setButtons([{
            label: 'Close',
            action: () => this.close(global.get_current_time()),
            key: Clutter.KEY_Escape,
            default: true,
        }]);
    }

    setRows(rows) {
        this._rows = rows;
        // An account that disappeared must not stay hidden-by-id forever.
        const live = new Set(rows.map(row => row.id));
        for (const id of [...this._hiddenAccounts]) {
            if (!live.has(id))
                this._hiddenAccounts.delete(id);
        }
        if (this._accountId !== null && !live.has(this._accountId))
            this._mode = 'overview';
        if (this.state === ModalDialog.State.OPENED)
            this._reload({keepRange: true});
    }

    showOverview() {
        this._mode = 'overview';
        this._accountId = null;
        this._measureActive = false;
        this._measurePoints = [];
        this._selectedCycleIndex = null;
        this._resetRange();
        this.open();
        this._reload();
    }

    showAccount(accountId, accountName, window = WINDOW.fiveHour) {
        this._mode = 'detail';
        this._accountId = accountId;
        this._accountName = accountName;
        this._window = window;
        this._measureActive = false;
        this._measurePoints = [];
        this._selectedCycleIndex = null;
        this._resetRange();
        this.open();
        this._reload();
    }

    _resetRange() {
        const now = Date.now();
        this._chart.setRange({fromMs: now - DEFAULT_SPAN_MS, toMs: now});
    }

    _selectWindow(window) {
        this._window = window;
        this._selectedCycleIndex = null;
        this._measurePoints = [];
        this._reload({keepRange: true});
    }

    _toggleMeasure() {
        this._measureActive = !this._measureActive;
        this._measureButton.checked = this._measureActive;
        this._measurePoints = [];
        this._chart.setMeasure({active: this._measureActive, points: []});
        this._updateReadout();
    }

    _showAll() {
        this._chart.setRange(fullRange(this._chart.allEntries()));
        this._reload({keepRange: true});
    }

    _onPointPicked(ms) {
        const nearest = nearestEntry(this._chart.allEntries(), ms);
        if (!nearest)
            return;
        // A third pick starts a fresh pair rather than silently replacing one
        // of the existing two.
        if (this._measurePoints.length >= 2)
            this._measurePoints = [];
        this._measurePoints.push(nearest);
        this._chart.setMeasure({active: true, points: this._measurePoints});
        this._updateReadout();
    }

    _onHover(ms) {
        this._hoverMs = ms < 0 ? null : ms;
        this._updateReadout();
    }

    _updateReadout() {
        if (this._measurePoints.length === 2) {
            const [a, b] = this._measurePoints;
            const m = measure(a, b);
            const minutes = Math.round(m.deltaMs / 60_000);
            const rate = m.perHour === null ? '—' : `${m.perHour.toFixed(1)}%/h`;
            this._readout.text =
                `Δ ${m.deltaUtilization.toFixed(1)}%  over ${minutes} min  ·  ${rate}`;
            return;
        }
        if (this._measureActive) {
            this._readout.text = this._measurePoints.length === 0
                ? 'Measure: click two points on the chart.'
                : 'Measure: click a second point.';
            return;
        }
        if (this._hoverMs === null) {
            this._readout.text = this._mode === 'detail'
                ? 'Scroll to zoom, drag to pan.'
                : 'Scroll to zoom, drag to pan. Click a legend entry to hide an account.';
            return;
        }
        const entry = nearestEntry(this._chart.allEntries(), this._hoverMs);
        if (!entry) {
            this._readout.text = '';
            return;
        }
        const when = new Date(entry.recordedAtMs);
        const hh = String(when.getHours()).padStart(2, '0');
        const mm = String(when.getMinutes()).padStart(2, '0');
        this._readout.text = `${hh}:${mm}  ·  ${entry.utilization.toFixed(1)}%`;
    }

    _reload({keepRange = true} = {}) {
        void keepRange;
        const range = this._chart.range;
        const isDetail = this._mode === 'detail';

        this._title.text = isDetail ? this._accountName : 'Overview';
        this._allAccountsButton.visible = isDetail;
        this._measureButton.visible = isDetail;
        this._totalButton.visible = !isDetail;
        this._legend.visible = !isDetail;
        this._cyclesBox.visible = isDetail;
        for (const [key, button] of this._windowButtons)
            button.checked = key === this._window;

        const series = isDetail ? this._detailSeries(range) : this._overviewSeries(range);
        this._chart.setSeries(series);

        const hasData = series.some(s => s.entries.length > 0);
        this._empty.visible = !hasData;
        this._empty.text = 'No history yet — usage is logged as the panel refreshes.';
        this._chart.visible = hasData;

        this._chart.setTotal(!isDetail && this._showTotal
            ? totalSeries(new Map(series.map(s => [s.key, s.entries])))
            : null);

        // 5h reset markers are only meaningful under the 7d line, which is
        // exactly when AccountDetailViewModel populates them.
        this._chart.setMarkers(isDetail && this._window === WINDOW.sevenDay
            ? this._store.log
                .resetCycles({accountId: this._accountId, window: WINDOW.fiveHour})
                .map(c => c.resetsAtMs)
                .filter(ms => ms >= range.fromMs && ms <= range.toMs)
                .sort((a, b) => a - b)
            : []);

        if (isDetail)
            this._rebuildCycles();
        else
            this._rebuildLegend(series);

        this._updateReadout();
    }

    _detailSeries(range) {
        const log = this._store.log;
        const query = {accountId: this._accountId, window: this._window};

        let entries;
        if (this._selectedCycleIndex !== null) {
            const cycles = log.resetCycles(query);
            const cycle = cycles[this._selectedCycleIndex];
            entries = cycle
                ? log.logs({
                    ...query,
                    // The -1s matches AccountDetailViewModel's
                    // `from: cycle.firstRecordedAt.addingTimeInterval(-1)`.
                    fromMs: cycle.firstRecordedAtMs - 1000,
                    toMs: cycle.resetsAtMs,
                })
                : [];
        } else {
            const inRange = log.logs({...query, fromMs: range.fromMs, toMs: range.toMs});
            // The two samples either side keep the line running to the frame
            // edge instead of stopping at the first in-range point.
            const before = log.logsBefore({...query, beforeMs: range.fromMs, limit: BUFFER_SAMPLES});
            const after = log.logsAfter({...query, afterMs: range.toMs, limit: BUFFER_SAMPLES});
            entries = [...before, ...inRange, ...after];
        }

        return [{
            key: this._accountId,
            color: SERIES_COLORS[0],
            entries: withResetTransitions(entries),
        }];
    }

    _overviewSeries(range) {
        const log = this._store.log;
        return this._rows
            .filter(row => !this._hiddenAccounts.has(row.id))
            .map((row, index) => ({
                key: row.id,
                label: row.email ?? row.name,
                color: SERIES_COLORS[index % SERIES_COLORS.length],
                entries: withResetTransitions(log.logs({
                    accountId: row.id,
                    window: this._window,
                    fromMs: range.fromMs,
                    toMs: range.toMs,
                })),
            }));
    }

    _rebuildLegend() {
        this._legend.remove_all_children();
        // Built from every row, not just the visible series, so a hidden
        // account keeps a legend entry to switch back on.
        this._rows.forEach((row, index) => {
            const color = SERIES_COLORS[index % SERIES_COLORS.length];
            const hidden = this._hiddenAccounts.has(row.id);
            const content = new St.BoxLayout({style_class: 'claude-legend-entry-content'});
            content.add_child(swatch(color));
            content.add_child(new St.Label({
                text: row.email ?? row.name,
                y_align: Clutter.ActorAlign.CENTER,
            }));
            const button = new St.Button({
                child: content,
                style_class: hidden ? 'claude-legend-entry claude-legend-off' : 'claude-legend-entry',
                can_focus: true,
            });
            button.connect('clicked', () => {
                if (this._hiddenAccounts.has(row.id))
                    this._hiddenAccounts.delete(row.id);
                else
                    this._hiddenAccounts.add(row.id);
                this._reload({keepRange: true});
            });
            this._legend.add_child(button);
        });
    }

    _rebuildCycles() {
        this._cyclesBox.remove_all_children();
        const cycles = this._store.log.resetCycles({
            accountId: this._accountId,
            window: this._window,
        });
        if (cycles.length === 0)
            return;

        const header = new St.BoxLayout({style_class: 'claude-cycles-header'});
        header.add_child(new St.Label({text: 'Reset cycles', y_align: Clutter.ActorAlign.CENTER}));
        header.add_child(new St.Widget({x_expand: true}));
        const clear = new St.Button({label: 'All cycles', style_class: 'claude-toolbar-button'});
        clear.connect('clicked', () => {
            this._selectedCycleIndex = null;
            this._reload({keepRange: true});
        });
        clear.visible = this._selectedCycleIndex !== null;
        header.add_child(clear);
        this._cyclesBox.add_child(header);

        const totalSeconds = this._window === WINDOW.fiveHour ? 18000 : 604800;
        // Only the most recent handful: the list is a shortcut to a cycle, not
        // a log viewer, and the dialog has a fixed height.
        cycles.slice(0, 6).forEach((cycle, index) => {
            const selected = this._selectedCycleIndex === index;
            const label =
                `${formatResetTime(new Date(cycle.resetsAtMs), totalSeconds, new Date())}` +
                `   peak ${cycle.peakUtilization.toFixed(0)}%   ${cycle.dataPointCount} samples`;
            // An St.Button centres its label; the cycle list reads as a list
            // only when the rows start at the same x as the header above them.
            const button = new St.Button({
                style_class: selected ? 'claude-cycle-row claude-cycle-selected' : 'claude-cycle-row',
                can_focus: true,
                x_expand: true,
                child: new St.Label({text: label, x_align: Clutter.ActorAlign.START}),
            });
            button.connect('clicked', () => {
                this._selectedCycleIndex = selected ? null : index;
                this._reload({keepRange: true});
            });
            this._cyclesBox.add_child(button);
        });
    }
});

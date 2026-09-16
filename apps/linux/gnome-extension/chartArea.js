// The Cairo layer for the usage charts. Like ring.js, it computes nothing:
// every coordinate comes from lib/chart.js. Stands in for SwiftUI Charts,
// which has no GNOME equivalent.

import Cairo from 'gi://cairo';
import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import Pango from 'gi://Pango';
import PangoCairo from 'gi://PangoCairo';
import St from 'gi://St';

import {
    makeScale, Y_TICKS, timeTicks, formatTick, nearestEntry, segments,
    zoomRange, panRange,
} from './lib/chart.js';

const PADDING = {left: 44, right: 16, top: 16, bottom: 28};
const GRID_ALPHA = 0.12;
const AXIS_TEXT_ALPHA = 0.55;
const LINE_WIDTH = 2;

// OverviewChartView.swift:22-24's palette, in the same order, so a given
// account keeps the same colour across both implementations.
export const SERIES_COLORS = [
    {r: 1.00, g: 0.58, b: 0.00},  // orange
    {r: 0.20, g: 0.78, b: 0.94},  // cyan
    {r: 0.20, g: 0.78, b: 0.35},  // green
    {r: 0.69, g: 0.32, b: 0.87},  // purple
    {r: 1.00, g: 0.18, b: 0.57},  // pink
    {r: 0.04, g: 0.52, b: 1.00},  // blue
    {r: 1.00, g: 0.80, b: 0.00},  // yellow
    {r: 0.00, g: 0.78, b: 0.75},  // mint
    {r: 0.35, g: 0.34, b: 0.84},  // indigo
    {r: 1.00, g: 0.27, b: 0.23},  // red
];

// OverviewChartView.swift:20's Color.white.opacity(0.5) for the total line.
const TOTAL_COLOR = {r: 1, g: 1, b: 1};

function drawText(cr, text, x, y, {size = 10, alpha = 1, align = 'left'} = {}) {
    const layout = PangoCairo.create_layout(cr);
    const desc = Pango.FontDescription.from_string(`Sans ${size}`);
    layout.set_font_description(desc);
    layout.set_text(text, -1);
    const [w, h] = layout.get_pixel_size();
    let originX = x;
    if (align === 'center')
        originX = x - w / 2;
    else if (align === 'right')
        originX = x - w;
    cr.setSourceRGBA(1, 1, 1, alpha);
    cr.moveTo(originX, y - h / 2);
    PangoCairo.show_layout(cr, layout);
}

export const ChartArea = GObject.registerClass({
    Signals: {
        // Emitted with the nearest entry's recorded time, or -1 when the
        // pointer leaves the plot, so the owner can render its own readout.
        'hover-changed': {param_types: [GObject.TYPE_DOUBLE]},
        'range-changed': {param_types: [GObject.TYPE_DOUBLE, GObject.TYPE_DOUBLE]},
        'point-picked': {param_types: [GObject.TYPE_DOUBLE]},
    },
}, class ChartArea extends St.DrawingArea {
    _init(params = {}) {
        super._init({
            style_class: 'claude-chart-area',
            reactive: true,
            ...params,
        });

        // Each series is {key, color, entries}; a single-account chart just has
        // one. `total` is the optional summed line the Overview can show.
        this._series = [];
        this._total = null;
        this._range = {fromMs: Date.now() - 86_400_000, toMs: Date.now()};
        this._hoverMs = null;
        this._markers = [];
        this._measurePoints = [];
        this._measureActive = false;
        this._dragAnchorMs = null;

        this.connect('repaint', () => this._repaint());
        this.connect('motion-event', (_a, event) => this._onMotion(event));
        this.connect('leave-event', () => {
            this._hoverMs = null;
            this.emit('hover-changed', -1);
            this.queue_repaint();
            return Clutter.EVENT_PROPAGATE;
        });
        this.connect('scroll-event', (_a, event) => this._onScroll(event));
        this.connect('button-press-event', (_a, event) => this._onPress(event));
        this.connect('button-release-event', () => {
            this._dragAnchorMs = null;
            return Clutter.EVENT_PROPAGATE;
        });
    }

    setSeries(series) {
        this._series = series;
        this.queue_repaint();
    }

    setTotal(total) {
        this._total = total;
        this.queue_repaint();
    }

    setRange(range) {
        this._range = range;
        this.queue_repaint();
    }

    get range() {
        return this._range;
    }

    // Vertical rules at 5-hour reset instants, drawn when the 7d window is
    // shown (AccountDetailViewModel.fiveHourResetMarkers).
    setMarkers(markersMs) {
        this._markers = markersMs;
        this.queue_repaint();
    }

    setMeasure({active, points}) {
        this._measureActive = active;
        this._measurePoints = points;
        this.queue_repaint();
    }

    // The union of every series, used for hover resolution and "Show All".
    allEntries() {
        return this._series.flatMap(s => s.entries);
    }

    _scale() {
        const [width, height] = this.get_surface_size();
        return makeScale({
            fromMs: this._range.fromMs,
            toMs: this._range.toMs,
            width, height, padding: PADDING,
        });
    }

    _onMotion(event) {
        const [x, y] = event.get_coords();
        const [ax, ay] = this.get_transformed_position();
        const scale = this._scale();
        const localX = x - ax;
        const localY = y - ay;
        if (localX < scale.left || localX > scale.right || localY < scale.top || localY > scale.bottom) {
            this._hoverMs = null;
            this.emit('hover-changed', -1);
            this.queue_repaint();
            return Clutter.EVENT_PROPAGATE;
        }

        const ms = scale.msAt(localX);
        if (this._dragAnchorMs !== null) {
            // Dragging pans: the instant grabbed stays under the pointer.
            this.setRange(panRange(this._range, this._dragAnchorMs - ms));
            this.emit('range-changed', this._range.fromMs, this._range.toMs);
            return Clutter.EVENT_STOP;
        }

        this._hoverMs = ms;
        const nearest = nearestEntry(this.allEntries(), ms);
        this.emit('hover-changed', nearest ? nearest.recordedAtMs : -1);
        this.queue_repaint();
        return Clutter.EVENT_PROPAGATE;
    }

    _onScroll(event) {
        const direction = event.get_scroll_direction();
        if (direction !== Clutter.ScrollDirection.UP && direction !== Clutter.ScrollDirection.DOWN)
            return Clutter.EVENT_PROPAGATE;
        const [x] = event.get_coords();
        const [ax] = this.get_transformed_position();
        const scale = this._scale();
        const anchorMs = scale.msAt(Math.min(scale.right, Math.max(scale.left, x - ax)));
        const factor = direction === Clutter.ScrollDirection.UP ? 0.8 : 1.25;
        this.setRange(zoomRange(this._range, factor, anchorMs));
        this.emit('range-changed', this._range.fromMs, this._range.toMs);
        return Clutter.EVENT_STOP;
    }

    _onPress(event) {
        const [x] = event.get_coords();
        const [ax] = this.get_transformed_position();
        const scale = this._scale();
        const ms = scale.msAt(x - ax);
        if (this._measureActive) {
            this.emit('point-picked', ms);
            return Clutter.EVENT_STOP;
        }
        this._dragAnchorMs = ms;
        return Clutter.EVENT_STOP;
    }

    _repaint() {
        const cr = this.get_context();
        const [width, height] = this.get_surface_size();
        const scale = this._scale();
        const spanMs = this._range.toMs - this._range.fromMs;

        cr.setOperator(Cairo.Operator.CLEAR);
        cr.paint();
        cr.setOperator(Cairo.Operator.OVER);

        // Gridlines and the y labels.
        cr.setLineWidth(1);
        for (const tick of Y_TICKS) {
            const y = Math.round(scale.y(tick)) + 0.5;
            cr.setSourceRGBA(1, 1, 1, GRID_ALPHA);
            cr.moveTo(scale.left, y);
            cr.lineTo(scale.right, y);
            cr.stroke();
            drawText(cr, `${tick}%`, scale.left - 6, scale.y(tick), {
                size: 8, alpha: AXIS_TEXT_ALPHA, align: 'right',
            });
        }

        // Time axis.
        for (const tick of timeTicks(this._range.fromMs, this._range.toMs)) {
            const x = Math.round(scale.x(tick)) + 0.5;
            cr.setSourceRGBA(1, 1, 1, GRID_ALPHA * 0.6);
            cr.moveTo(x, scale.top);
            cr.lineTo(x, scale.bottom);
            cr.stroke();
            drawText(cr, formatTick(tick, spanMs), scale.x(tick), scale.bottom + 12, {
                size: 8, alpha: AXIS_TEXT_ALPHA, align: 'center',
            });
        }

        // 5-hour reset markers, dashed so they read as annotation not data.
        if (this._markers.length > 0) {
            cr.setDash([3, 3], 0);
            cr.setSourceRGBA(1, 1, 1, 0.25);
            for (const markerMs of this._markers) {
                if (markerMs < this._range.fromMs || markerMs > this._range.toMs)
                    continue;
                const x = Math.round(scale.x(markerMs)) + 0.5;
                cr.moveTo(x, scale.top);
                cr.lineTo(x, scale.bottom);
                cr.stroke();
            }
            cr.setDash([], 0);
        }

        cr.setLineWidth(LINE_WIDTH);
        cr.setLineJoin(Cairo.LineJoin.ROUND);
        cr.setLineCap(Cairo.LineCap.ROUND);

        for (const series of this._series)
            this._strokeSeries(cr, scale, series.entries, series.color, 1);

        if (this._total)
            this._strokeSeries(cr, scale, this._total, TOTAL_COLOR, 0.5);

        this._drawHover(cr, scale, height);
        this._drawMeasure(cr, scale);

        // The frame, last, so the data never paints over it.
        cr.setLineWidth(1);
        cr.setSourceRGBA(1, 1, 1, GRID_ALPHA);
        cr.rectangle(scale.left + 0.5, scale.top + 0.5, scale.plotWidth, scale.plotHeight);
        cr.stroke();

        cr.$dispose();
        void width;
    }

    _strokeSeries(cr, scale, entries, color, alpha) {
        cr.setSourceRGBA(color.r, color.g, color.b, alpha);
        for (const segment of segments(entries)) {
            if (segment.length === 1) {
                // A lone sample would stroke nothing; draw it as a dot so a
                // sparse series is still visible.
                cr.arc(scale.x(segment[0].recordedAtMs), scale.y(segment[0].utilization), LINE_WIDTH, 0, Math.PI * 2);
                cr.fill();
                continue;
            }
            segment.forEach((entry, index) => {
                const x = scale.x(entry.recordedAtMs);
                const y = scale.y(entry.utilization);
                if (index === 0)
                    cr.moveTo(x, y);
                else
                    cr.lineTo(x, y);
            });
            cr.stroke();
        }
    }

    _drawHover(cr, scale) {
        if (this._hoverMs === null)
            return;
        const nearest = nearestEntry(this.allEntries(), this._hoverMs);
        if (!nearest)
            return;
        const x = Math.round(scale.x(nearest.recordedAtMs)) + 0.5;
        cr.setLineWidth(1);
        cr.setSourceRGBA(1, 1, 1, 0.35);
        cr.moveTo(x, scale.top);
        cr.lineTo(x, scale.bottom);
        cr.stroke();

        cr.setSourceRGBA(1, 1, 1, 0.9);
        cr.arc(scale.x(nearest.recordedAtMs), scale.y(nearest.utilization), 3, 0, Math.PI * 2);
        cr.fill();
    }

    _drawMeasure(cr, scale) {
        if (this._measurePoints.length === 0)
            return;
        cr.setLineWidth(1);
        cr.setSourceRGBA(1, 0.85, 0.2, 0.9);
        for (const point of this._measurePoints) {
            const x = Math.round(scale.x(point.recordedAtMs)) + 0.5;
            cr.moveTo(x, scale.top);
            cr.lineTo(x, scale.bottom);
            cr.stroke();
            cr.arc(scale.x(point.recordedAtMs), scale.y(point.utilization), 4, 0, Math.PI * 2);
            cr.fill();
        }
        if (this._measurePoints.length === 2) {
            const [a, b] = this._measurePoints;
            cr.setDash([4, 3], 0);
            cr.moveTo(scale.x(a.recordedAtMs), scale.y(a.utilization));
            cr.lineTo(scale.x(b.recordedAtMs), scale.y(b.utilization));
            cr.stroke();
            cr.setDash([], 0);
        }
    }
});

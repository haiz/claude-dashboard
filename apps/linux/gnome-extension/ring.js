// A Cairo layer that computes nothing: every angle, fraction and font size
// comes from lib/geometry.js, which is unit-tested. GJS exposes no pixel
// access on a Cairo image surface, so this file is verified by eye.

import Cairo from 'gi://cairo';
import GObject from 'gi://GObject';
import St from 'gi://St';

import {usageColor, countdownColor} from './lib/colors.js';
import {
    METRICS, TAU, progressArc, countdownSegments, segmentCountFor, segmentRange,
    toAngle, ringRadius, ringCenter,
} from './lib/geometry.js';

const TRACK_ALPHA = 0.2;
const COUNTDOWN_TRACK_ALPHA = 0.08;

function foreground(actor) {
    const c = actor.get_theme_node().get_foreground_color();
    return {r: c.red / 255, g: c.green / 255, b: c.blue / 255};
}

export const UsageRing = GObject.registerClass(
class UsageRing extends St.DrawingArea {
    _init(diameter = METRICS.largeDiameter, lineWidth = METRICS.largeLineWidth) {
        super._init({
            width: diameter,
            height: diameter,
            style_class: 'claude-usage-ring',
        });
        this._diameter = diameter;
        this._lineWidth = lineWidth;
        this._utilization = 0;
        this.connect('repaint', () => this._repaint());
    }

    setValue(utilization) {
        this._utilization = utilization;
        this.queue_repaint();
    }

    _repaint() {
        const cr = this.get_context();
        const d = this._diameter;
        const radius = ringRadius(d, this._lineWidth);
        const cx = ringCenter(d), cy = ringCenter(d);
        const fg = foreground(this);

        cr.setLineWidth(this._lineWidth);
        cr.setLineCap(Cairo.LineCap.BUTT); // A full circle, so the caps never show.
        cr.setSourceRGBA(fg.r, fg.g, fg.b, TRACK_ALPHA);
        cr.arc(cx, cy, radius, 0, TAU);
        cr.stroke();

        const arc = progressArc(this._utilization);
        if (!arc.empty) {
            const c = usageColor(this._utilization);
            cr.setLineCap(Cairo.LineCap.ROUND); // matches SwiftUI's StrokeStyle(lineCap: .round)
            cr.setSourceRGBA(c.r, c.g, c.b, 0.92);
            cr.arc(cx, cy, radius, arc.start, arc.end);
            cr.stroke();
        }

        cr.$dispose();
    }
});

export const CountdownRing = GObject.registerClass(
class CountdownRing extends St.DrawingArea {
    _init(diameter = METRICS.smallDiameter, lineWidth = METRICS.smallLineWidth) {
        super._init({
            width: diameter,
            height: diameter,
            style_class: 'claude-countdown-ring',
        });
        this._diameter = diameter;
        this._lineWidth = lineWidth;
        this._remainingSeconds = 0;
        this._totalSeconds = 18000;
        this.connect('repaint', () => this._repaint());
    }

    setValue(remainingSeconds, totalSeconds) {
        this._remainingSeconds = remainingSeconds;
        this._totalSeconds = totalSeconds;
        this.queue_repaint();
    }

    _repaint() {
        const cr = this.get_context();
        const d = this._diameter;
        const radius = ringRadius(d, this._lineWidth);
        const cx = ringCenter(d), cy = ringCenter(d);
        const fg = foreground(this);
        const count = segmentCountFor(this._totalSeconds);

        cr.setLineWidth(this._lineWidth);
        cr.setLineCap(Cairo.LineCap.BUTT); // so the gaps between segments stay square

        cr.setSourceRGBA(fg.r, fg.g, fg.b, COUNTDOWN_TRACK_ALPHA);
        for (let i = 0; i < count; i++) {
            const {start, end} = segmentRange(i, d, count);
            cr.newSubPath();
            cr.arc(cx, cy, radius, toAngle(start), toAngle(end));
            cr.stroke();
        }

        const c = countdownColor(this._remainingSeconds, this._totalSeconds);
        cr.setSourceRGBA(c.r, c.g, c.b, 1.0);
        for (const seg of countdownSegments(this._remainingSeconds, this._totalSeconds, d, count)) {
            cr.newSubPath();
            cr.arc(cx, cy, radius, toAngle(seg.start), toAngle(seg.end));
            cr.stroke();
        }

        cr.$dispose();
    }
});

export function panelRing() {
    return new UsageRing(16, 2.5);
}

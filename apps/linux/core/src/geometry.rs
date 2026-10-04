//! Ring-gauge geometry, ported verbatim from `apps/linux/lib/geometry.js`.

use std::f64::consts::PI;

pub const TAU: f64 = PI * 2.0;
pub const START_ANGLE: f64 = -PI / 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub large_diameter: f64,
    pub small_diameter: f64,
    pub large_line_width: f64,
    pub small_line_width: f64,
    pub window_label_font_size: f64,
    pub reset_label_font_size: f64,
    pub animal_font_size: f64,
    pub ring_gap: f64,
    pub column_gap: f64,
}

pub const METRICS: Metrics = Metrics {
    large_diameter: 52.0,
    small_diameter: 34.0,
    large_line_width: 6.0,
    small_line_width: 4.0,
    window_label_font_size: 10.0,
    reset_label_font_size: 8.0,
    animal_font_size: 10.0,
    ring_gap: 5.0,
    column_gap: 5.0,
};

pub const METRICS_REGULAR: Metrics = Metrics {
    large_diameter: 68.0,
    small_diameter: 44.0,
    large_line_width: 8.0,
    small_line_width: 5.0,
    window_label_font_size: 13.0,
    reset_label_font_size: 8.0,
    animal_font_size: 10.0,
    ring_gap: 8.0,
    column_gap: 5.0,
};

pub fn metrics_for(is_compact: bool) -> Metrics {
    if is_compact { METRICS } else { METRICS_REGULAR }
}

pub fn fill_fraction(utilization: f64) -> f64 {
    (utilization.max(0.0) / 100.0).min(1.0)
}

/// (start_angle, end_angle)
pub fn progress_arc(utilization: f64) -> (f64, f64) {
    let fraction = fill_fraction(utilization);
    (START_ANGLE, START_ANGLE + fraction * TAU)
}

pub fn segment_count_for(total_seconds: f64) -> u32 {
    if total_seconds <= 18000.0 { 5 } else { 7 }
}

/// A two-pixel gap expressed as a fraction of the circumference.
pub fn gap_fraction(diameter: f64) -> f64 {
    2.0 / (PI * diameter)
}

pub fn segment_fraction(diameter: f64, segment_count: u32) -> f64 {
    let n = segment_count as f64;
    (1.0 - n * gap_fraction(diameter)) / n
}

pub fn segment_range(index: u32, diameter: f64, segment_count: u32) -> (f64, f64) {
    let gap = gap_fraction(diameter);
    let segment = segment_fraction(diameter, segment_count);
    let start = index as f64 * (segment + gap);
    (start, start + segment)
}

pub fn countdown_segments(
    remaining_s: f64,
    total_s: f64,
    diameter: f64,
    segment_count: u32,
) -> Vec<(f64, f64)> {
    let remaining = remaining_s.max(0.0);
    let fraction = if total_s > 0.0 { (remaining / total_s).min(1.0) } else { 0.0 };
    if fraction <= 0.0 {
        return Vec::new();
    }
    let arc_range = 1.0 - gap_fraction(diameter);
    let fill_start = (1.0 - fraction) * arc_range;

    let mut out = Vec::new();
    for i in 0..segment_count {
        let (start, end) = segment_range(i, diameter, segment_count);
        if end > fill_start {
            out.push((start.max(fill_start), end));
        }
    }
    out
}

pub fn to_angle(fraction: f64) -> f64 {
    START_ANGLE + fraction * TAU
}

pub fn ring_radius(diameter: f64, line_width: f64) -> f64 {
    (diameter - line_width) / 2.0
}

pub fn ring_center(diameter: f64) -> f64 {
    diameter / 2.0
}

pub fn percent_font_size(diameter: f64) -> f64 {
    diameter * 0.33
}

pub fn percent_sign_font_size(diameter: f64) -> f64 {
    diameter * 0.20
}

pub fn countdown_font_size(diameter: f64) -> f64 {
    diameter * 0.24
}

#[cfg(test)]
mod tests {
    use super::*;
    const EPS: f64 = 1e-9;
    fn close(a: f64, b: f64) -> bool { (a - b).abs() < EPS }

    #[test]
    fn fill_fraction_clamps() {
        assert!(close(fill_fraction(0.0), 0.0));
        assert!(close(fill_fraction(100.0), 1.0));
        assert!(close(fill_fraction(50.0), 0.5));
        assert!(close(fill_fraction(-10.0), 0.0));
        assert!(close(fill_fraction(250.0), 1.0));
    }

    #[test]
    fn angles() {
        assert!(close(START_ANGLE, -std::f64::consts::FRAC_PI_2));
        assert!(close(TAU, 2.0 * std::f64::consts::PI));
        assert!(close(to_angle(0.0), START_ANGLE));
        assert!(close(to_angle(0.5), std::f64::consts::FRAC_PI_2));
        let (s, e) = progress_arc(100.0);
        assert!(close(s, START_ANGLE));
        assert!(close(e - s, TAU));
        let (s, e) = progress_arc(0.0);
        assert!(close(e, s));
    }

    #[test]
    fn segment_thresholds() {
        assert_eq!(segment_count_for(18000.0), 5);
        assert_eq!(segment_count_for(18000.5), 7);
        assert_eq!(segment_count_for(604800.0), 7);
        assert_eq!(segment_count_for(0.0), 5);
    }

    #[test]
    fn countdown_full_empty_half() {
        assert!(countdown_segments(0.0, 18000.0, 52.0, 5).is_empty());
        assert!(countdown_segments(-5.0, 18000.0, 52.0, 5).is_empty());
        assert!(countdown_segments(10.0, 0.0, 52.0, 5).is_empty());
        let full = countdown_segments(18000.0, 18000.0, 52.0, 5);
        assert_eq!(full.len(), 5);
        assert!(close(full[0].0, 0.0));
        let gap = 2.0 / (std::f64::consts::PI * 52.0);
        let seg = (1.0 - 5.0 * gap) / 5.0;
        assert!(close(full[0].1, seg));
        assert!(close(full[1].0, seg + gap));
        // half remaining: fill_start = 0.5*(1-gap); segments ending before it drop out
        let half = countdown_segments(9000.0, 18000.0, 52.0, 5);
        assert!(half.len() < 5 && half.len() >= 2);
        let fill_start = 0.5 * (1.0 - gap);
        assert!(close(half[0].0.max(fill_start), half[0].0));
        for r in &half { assert!(r.1 > fill_start); }
        // literal pin: 4 segments of 5 at 52 diameter, fraction 1 -> last end ~ 1 - gap
        assert!(close(full[4].1, 1.0 - gap));
    }

    #[test]
    fn sizes_literal() {
        assert!(close(ring_radius(52.0, 6.0), 23.0));
        assert!(close(ring_center(52.0), 26.0));
        assert!(close(percent_font_size(52.0), 17.16));
        assert!(close(percent_sign_font_size(50.0), 10.0));
        assert!(close(countdown_font_size(50.0), 12.0));
    }

    #[test]
    fn metrics() {
        let c = metrics_for(true);
        assert!(close(c.large_diameter, 52.0));
        assert!(close(c.small_diameter, 34.0));
        assert!(close(c.large_line_width, 6.0));
        assert!(close(c.small_line_width, 4.0));
        assert!(close(c.window_label_font_size, 10.0));
        assert!(close(c.ring_gap, 5.0));
        let r = metrics_for(false);
        assert!(close(r.large_diameter, 68.0));
        assert!(close(r.small_diameter, 44.0));
        assert!(close(r.large_line_width, 8.0));
        assert!(close(r.small_line_width, 5.0));
        assert!(close(r.window_label_font_size, 13.0));
        assert!(close(r.reset_label_font_size, 8.0));
        assert!(close(r.animal_font_size, 10.0));
        assert!(close(r.ring_gap, 8.0));
        assert!(close(r.column_gap, 5.0));
    }
}

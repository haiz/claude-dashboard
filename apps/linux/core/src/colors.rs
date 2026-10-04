//! Colour math ported from `apps/linux/lib/colors.js` (and the macOS
//! `AccountAvatar` palette). Channels are 0.0..=1.0.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

const COUNTDOWN_BLUE: Rgb = Rgb { r: 74.0 / 255.0, g: 144.0 / 255.0, b: 217.0 / 255.0 };

/// SwiftUI system colours (light appearance) in the same order as
/// `AccountAvatar.palette`: blue, orange, green, pink, teal, indigo, red,
/// mint, purple, brown, cyan, yellow.
const AVATAR_PALETTE: [(u8, u8, u8); 12] = [
    (0, 122, 255),
    (255, 149, 0),
    (52, 199, 89),
    (255, 45, 85),
    (48, 176, 199),
    (88, 86, 214),
    (255, 59, 48),
    (0, 199, 190),
    (175, 82, 222),
    (162, 132, 94),
    (50, 173, 230),
    (255, 204, 0),
];

pub fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Rgb {
    let h = (hue % 1.0 + 1.0) % 1.0;
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - f * saturation);
    let t = value * (1.0 - (1.0 - f) * saturation);
    match (i as i64) % 6 {
        0 => Rgb { r: value, g: t, b: p },
        1 => Rgb { r: q, g: value, b: p },
        2 => Rgb { r: p, g: value, b: t },
        3 => Rgb { r: p, g: q, b: value },
        4 => Rgb { r: t, g: p, b: value },
        _ => Rgb { r: value, g: p, b: q },
    }
}

pub fn usage_color(utilization: f64) -> Rgb {
    let hue = (120.0 * (1.0 - utilization / 100.0)).clamp(0.0, 120.0) / 360.0;
    hsv_to_rgb(hue, 0.7, 0.85)
}

pub fn countdown_color(remaining_s: f64, total_s: f64) -> Rgb {
    if remaining_s.is_nan() || total_s.is_nan() || remaining_s <= 0.0 || total_s <= 0.0 {
        return hsv_to_rgb(120.0 / 360.0, 0.7, 0.85);
    }
    let fraction = (remaining_s / total_s).min(1.0);
    if fraction > 0.3 {
        return COUNTDOWN_BLUE;
    }
    let green_intensity = 1.0 - fraction / 0.3;
    hsv_to_rgb(120.0 / 360.0, 0.6 * green_intensity + 0.1, 0.5 + 0.35 * green_intensity)
}

/// Stable per-account colour. macOS sums the 16 bytes of the account UUID and
/// takes it modulo the 12-colour palette; a seed that parses as a UUID gets
/// exactly that colour. Any other seed (e.g. an email) sums its UTF-8 bytes.
pub fn avatar_color(seed: &str) -> Rgb {
    let sum: usize = match parse_uuid_bytes(seed) {
        Some(bytes) => bytes.iter().map(|b| *b as usize).sum(),
        None => seed.bytes().map(|b| b as usize).sum(),
    };
    let (r, g, b) = AVATAR_PALETTE[sum % AVATAR_PALETTE.len()];
    Rgb { r: r as f64 / 255.0, g: g as f64 / 255.0, b: b as f64 / 255.0 }
}

fn parse_uuid_bytes(s: &str) -> Option<[u8; 16]> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if s.len() != 36 || hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb) -> bool {
        (a.r - b.r).abs() < 1e-9 && (a.g - b.g).abs() < 1e-9 && (a.b - b.b).abs() < 1e-9
    }

    #[test]
    fn usage_endpoints() {
        assert!(close(usage_color(0.0), hsv_to_rgb(120.0 / 360.0, 0.7, 0.85)));
        assert!(close(usage_color(100.0), hsv_to_rgb(0.0, 0.7, 0.85)));
        assert!(close(usage_color(50.0), hsv_to_rgb(60.0 / 360.0, 0.7, 0.85)));
        assert!(close(usage_color(150.0), hsv_to_rgb(0.0, 0.7, 0.85)));
    }

    #[test]
    fn countdown_branches() {
        assert!(close(countdown_color(50.0, 100.0), COUNTDOWN_BLUE));
        assert!(close(countdown_color(0.0, 100.0), hsv_to_rgb(120.0 / 360.0, 0.7, 0.85)));
        let gi = 1.0 - 0.1 / 0.3;
        assert!(close(
            countdown_color(10.0, 100.0),
            hsv_to_rgb(120.0 / 360.0, 0.6 * gi + 0.1, 0.5 + 0.35 * gi)
        ));
    }

    #[test]
    fn avatar_deterministic_and_varied() {
        assert_eq!(avatar_color("a@x.com"), avatar_color("a@x.com"));
        assert_ne!(avatar_color("a"), avatar_color("b"));
    }

    #[test]
    fn avatar_matches_macos_uuid_sum() {
        // bytes 00..0f sum to 120; 120 % 12 == 0 -> blue
        let c = avatar_color("00010203-0405-0607-0809-0a0b0c0d0e0f");
        assert!(close(c, Rgb { r: 0.0, g: 122.0 / 255.0, b: 1.0 }));
    }
}

//! Ports `apps/linux/lib/format.js` (countdown and reset-time strings).
//!
//! No locale or timezone lookups happen here. `format_reset_time` renders the
//! wall clock of the seconds it is given as-is; a caller wanting local time
//! passes unix seconds already shifted by the local UTC offset (only the
//! comparison against `now` is offset-invariant, so shift both or neither).
//! The five-hour label uses the en-US shape `h:mm AM/PM`.

const FIVE_HOUR_SECONDS: f64 = 18000.0;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub fn formatted_countdown(remaining_s: f64, total_s: f64) -> String {
    if remaining_s.is_nan() || remaining_s <= 0.0 {
        return "0:00".to_string();
    }
    let total = remaining_s.floor() as i64;
    if total_s <= FIVE_HOUR_SECONDS {
        let hours = total / 3600;
        let minutes = (total % 3600) / 60;
        let seconds = total % 60;
        return if hours > 0 {
            format!("{hours}:{minutes:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        };
    }
    let days = total / 86400;
    let hours = (total % 86400) / 3600;
    let minutes = (total % 3600) / 60;
    if days > 0 {
        format!("{days}d{hours}h")
    } else {
        format!("{hours}:{minutes:02}")
    }
}

pub fn format_reset_time(reset_unix_s: f64, total_s: f64, now_unix_s: f64) -> String {
    if reset_unix_s <= now_unix_s {
        return "now".to_string();
    }
    let reset = reset_unix_s.floor() as i64;

    if total_s <= FIVE_HOUR_SECONDS {
        let sod = reset.rem_euclid(86400);
        let h24 = sod / 3600;
        let minute = (sod % 3600) / 60;
        let hour = if h24 % 12 == 0 { 12 } else { h24 % 12 };
        let suffix = if h24 < 12 { "AM" } else { "PM" };
        return format!("{hour}:{minute:02} {suffix}");
    }

    // Round to the nearest ten minutes (half up), in whole seconds.
    let in_hour = reset.rem_euclid(3600);
    let target = (in_hour as f64 / 600.0).round() as i64 * 600;
    let rounded = reset + target - in_hour;

    let days = rounded.div_euclid(86400);
    let weekday = WEEKDAYS[((days + 4).rem_euclid(7)) as usize]; // epoch was a Thursday
    let sod = rounded.rem_euclid(86400);
    let h24 = sod / 3600;
    let minute = (sod % 3600) / 60;
    let hour = if h24 % 12 == 0 { 12 } else { h24 % 12 };
    let suffix = if h24 < 12 { "am" } else { "pm" };
    if minute == 0 {
        format!("{weekday} {hour}{suffix}")
    } else {
        format!("{weekday} {hour}:{minute:02}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIVE: f64 = 18000.0;
    const SEVEN: f64 = 604800.0;

    #[test]
    fn five_hour_countdown() {
        assert_eq!(formatted_countdown(3.0 * 3600.0 + 7.0 * 60.0 + 12.0, FIVE), "3:07");
        assert_eq!(formatted_countdown(7.0 * 60.0 + 5.0, FIVE), "7:05");
    }

    #[test]
    fn seven_day_countdown() {
        assert_eq!(formatted_countdown(5.0 * 86400.0 + 8.0 * 3600.0, SEVEN), "5d8h");
        assert_eq!(formatted_countdown(3.0 * 3600.0 + 4.0 * 60.0, SEVEN), "3:04");
    }

    #[test]
    fn elapsed_countdown_reads_zero() {
        assert_eq!(formatted_countdown(0.0, FIVE), "0:00");
        assert_eq!(formatted_countdown(-10.0, SEVEN), "0:00");
        assert_eq!(formatted_countdown(f64::NAN, SEVEN), "0:00");
    }

    #[test]
    fn passed_reset_reads_now() {
        assert_eq!(format_reset_time(1789124400.0, FIVE, 1789128000.0), "now");
    }

    #[test]
    fn five_hour_reset_is_clock_time() {
        assert_eq!(format_reset_time(1789139100.0, FIVE, 1789128000.0), "3:05 PM");
    }

    #[test]
    fn seven_day_rounds_up_across_midnight() {
        assert_eq!(format_reset_time(1789257540.0, SEVEN, 1789128000.0), "Sun 12am");
    }

    #[test]
    fn seven_day_drops_zero_minute() {
        assert_eq!(format_reset_time(1789304640.0, SEVEN, 1789128000.0), "Sun 1pm");
    }

    #[test]
    fn seven_day_keeps_nonzero_minute() {
        assert_eq!(format_reset_time(1789304820.0, SEVEN, 1789128000.0), "Sun 1:10pm");
    }
}

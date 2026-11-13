//! Time as whole seconds since the Unix epoch, read from the RFC 3339 stamps
//! that git and Board print, and shown as an age.

/// Seconds in a day.
pub const DAY: i64 = 86_400;

/// The seconds since the epoch of an RFC 3339 stamp such as
/// `2026-11-13T10:12:25Z` or `2026-11-13T10:12:25+01:00`; `None` when the text
/// is not one.
#[must_use]
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let number = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b' ')
    {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        rest = &fraction[digits..];
    }
    let offset = match rest.as_bytes() {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            let seconds = hours * 3600 + minutes * 60;
            if *sign == b'+' { seconds } else { -seconds }
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * DAY + hour * 3600 + minute * 60 + second - offset)
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// An age in the largest whole unit: `6d`, `5h` or `12m`.
#[must_use]
pub fn age(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= DAY {
        format!("{}d", seconds / DAY)
    } else if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}m", seconds / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_utc_and_offset_stamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-11-13T10:12:25Z"), Some(1_794_564_745));
        assert_eq!(
            parse_rfc3339("2026-11-13T11:12:25+01:00"),
            Some(1_794_564_745)
        );
        assert_eq!(
            parse_rfc3339("2026-11-13T10:12:25.123456Z"),
            Some(1_794_564_745)
        );
    }

    #[test]
    fn refuses_text_that_is_not_a_stamp() {
        assert_eq!(parse_rfc3339("yesterday"), None);
        assert_eq!(parse_rfc3339("2026-13-13T10:12:25Z"), None);
        assert_eq!(parse_rfc3339("2026-11-13T10:12:25"), None);
    }

    #[test]
    fn ages_use_the_largest_whole_unit() {
        assert_eq!(age(6 * DAY + 5), "6d");
        assert_eq!(age(5 * 3600 + 9), "5h");
        assert_eq!(age(12 * 60), "12m");
        assert_eq!(age(-5), "0m");
    }
}

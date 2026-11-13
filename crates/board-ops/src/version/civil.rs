//! UTC timestamps as RFC 3339 text and as seconds since the Unix epoch.
//! board-ops reads and prints `min_board` build dates with it, and
//! crates/board-cli/build.rs prints the build date with it, because
//! deny.toml keeps jiff out of board-ops and out of build scripts
//! (docs/architecture/writes/minimum.md).

/// `seconds` as a UTC timestamp, such as `2026-11-13T10:00:00Z`.
pub(crate) fn utc(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rest / 3_600, rest % 3_600 / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The seconds since the Unix epoch that an RFC 3339 timestamp names, such
/// as `2026-11-13T10:00:00Z` or `2026-11-13T20:00:00+10:00`. Fractions of a
/// second are read and dropped. `None` when `text` is not a timestamp with
/// a zone, or names a day or time that does not exist.
pub(crate) fn parse_utc(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let number = |from: usize, to: usize| -> Option<i64> {
        let digits = bytes.get(from..to)?;
        digits
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(digits).ok()?.parse().ok())
            .flatten()
    };
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if bytes.len() < 20
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || separators.iter().any(|&(at, byte)| bytes[at] != byte)
    {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let mut rest = &bytes[19..];
    if let [b'.', tail @ ..] = rest {
        let digits = tail.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        rest = &tail[digits..];
    }
    let offset = match rest {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), ..] if rest.len() == 6 && rest[3] == b':' => {
            let at = |from: usize| {
                let digits = rest.get(from..from + 2)?;
                digits
                    .iter()
                    .all(u8::is_ascii_digit)
                    .then(|| std::str::from_utf8(digits).ok()?.parse::<i64>().ok())
                    .flatten()
            };
            let (hours, minutes) = (at(1)?, at(4)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours * 3_600 + minutes * 60;
            if *sign == b'-' { -seconds } else { seconds }
        }
        _ => return None,
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day < 1 || day > days_in || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// Howard Hinnant's days-to-civil algorithm, for the proleptic Gregorian
/// calendar: the year, month and day of `days` since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// The inverse of `civil_from_days`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

//! The calendar days the plugin reads and writes: a day as `YYYY-MM-DD`, the
//! days between two, and the UTC day and time of the host's clock. A plugin
//! gets the clock as a count of seconds, so it needs no time zone database.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// A day of the proleptic Gregorian calendar, as the days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Date(i64);

impl Date {
    /// The day `year-month-day`, or `None` when the calendar has no such day.
    pub(crate) fn new(year: i64, month: i64, day: i64) -> Option<Self> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let last = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if !(1..=last).contains(&day) {
            return None;
        }
        // Days from civil, after Howard Hinnant's algorithm.
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let of_era = year.rem_euclid(400);
        let shifted = (month + 9) % 12;
        let of_year = (153 * shifted + 2) / 5 + day - 1;
        let of_day = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
        Some(Self(era * 146_097 + of_day - 719_468))
    }

    /// How many days before `self` the day `earlier` was; negative when it
    /// is later.
    pub(crate) fn days_since(self, earlier: Self) -> i32 {
        i32::try_from(self.0 - earlier.0).unwrap_or(i32::MAX)
    }

    /// The year, month and day.
    fn civil(self) -> (i64, i64, i64) {
        let shifted = self.0 + 719_468;
        let era = shifted.div_euclid(146_097);
        let of_era = shifted.rem_euclid(146_097);
        let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
        let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * of_year + 2) / 153;
        let day = of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        (year, month, day)
    }
}

impl FromStr for Date {
    type Err = ();

    /// Reads `YYYY-MM-DD` and nothing else.
    fn from_str(text: &str) -> Result<Self, ()> {
        let bytes = text.as_bytes();
        let digits = |range: std::ops::Range<usize>| -> Result<i64, ()> {
            let part = bytes.get(range).ok_or(())?;
            if !part.iter().all(u8::is_ascii_digit) {
                return Err(());
            }
            std::str::from_utf8(part)
                .map_err(|_| ())?
                .parse()
                .map_err(|_| ())
        };
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(());
        }
        Self::new(digits(0..4)?, digits(5..7)?, digits(8..10)?).ok_or(())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.civil();
        write!(out, "{year:04}-{month:02}-{day:02}")
    }
}

/// The UTC day of `now` and the seconds since that day began.
pub(crate) fn utc(now: SystemTime) -> (Date, u32) {
    let seconds = match now.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let of_day = u32::try_from(seconds.rem_euclid(86_400)).unwrap_or(0);
    (Date(seconds.div_euclid(86_400)), of_day)
}

/// `now` as `YYYYMMDDTHHMMSSZ`.
pub(crate) fn stamp(now: SystemTime) -> String {
    let (date, of_day) = utc(now);
    let (year, month, day) = date.civil();
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

#[cfg(test)]
mod tests;

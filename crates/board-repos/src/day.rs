//! The person's day, which starts at `day_start`, such as `07:00`, or at
//! midnight. Board reads `day_start` from its known boards file and
//! refuses a value that is not `HH:MM`. A note at 01:00 with a day that
//! starts at 07:00 belongs to yesterday.

use std::fmt;
use std::time::{Duration, SystemTime};

const DAY: i64 = 86_400;

/// How long after midnight the person's day starts, from
/// `Settings::day_start`; midnight without one. Board passes only `HH:MM`,
/// so any other text reads as midnight.
pub fn day_start(setting: Option<&str>) -> Duration {
    let parsed = setting.and_then(|text| {
        let (hours, minutes) = text.split_once(':')?;
        Some(hours.parse::<u64>().ok()? * 3600 + minutes.parse::<u64>().ok()? * 60)
    });
    Duration::from_secs(parsed.unwrap_or(0))
}

/// A date in the proleptic Gregorian calendar, shown as ISO 8601's
/// `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i64,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// The date `days` days after 1970-01-01, by Howard Hinnant's
    /// `civil_from_days`.
    fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        let small = |value: i64| u8::try_from(value).expect("a month or day is 1 to 31");
        Self {
            year,
            month: small(month),
            day: small(day),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// The date of the day `now` falls in, where the local clock is `offset`
/// seconds east of UTC, for a day that starts `start` after midnight.
pub fn today(now: SystemTime, offset: i32, start: Duration) -> Date {
    let seconds = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let start = i64::try_from(start.as_secs()).unwrap_or(0);
    let local = seconds
        .saturating_add(i64::from(offset))
        .saturating_sub(start);
    Date::from_days(local.div_euclid(DAY))
}

#[cfg(test)]
mod tests;

//! The times Board writes and reads: RFC 3339 text and the instants it names.

use std::time::{SystemTime, UNIX_EPOCH};

use jiff::Timestamp;

/// `time` as a jiff timestamp. A time before 1970 is the epoch and a time
/// after jiff's last timestamp is that timestamp.
fn clamped(time: SystemTime) -> Timestamp {
    match Timestamp::try_from(time) {
        Ok(at) => at.max(Timestamp::UNIX_EPOCH),
        Err(_) if time < UNIX_EPOCH => Timestamp::UNIX_EPOCH,
        Err(_) => Timestamp::MAX,
    }
}

/// `time` as RFC 3339 in UTC to the microsecond, such as
/// `2026-10-30T22:44:54.102864Z`. Every time from 1970 to 9999 has the same
/// width, so text order is time order.
pub fn utc(time: SystemTime) -> String {
    format!("{:.6}", clamped(time))
}

/// `time` as RFC 3339 in UTC to the second, such as `2026-11-01T09:00:00Z`.
pub fn utc_seconds(time: SystemTime) -> String {
    format!("{:.0}", clamped(time))
}

/// The moment an RFC 3339 `text` names, at any offset, or none when `text` is
/// no time.
pub fn moment(text: &str) -> Option<SystemTime> {
    text.parse::<Timestamp>().ok().map(SystemTime::from)
}

#[cfg(test)]
mod tests;

//! The times Board writes and reads: RFC 3339 text and the instants it names.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::Timestamp;

/// `time` as a jiff timestamp. A time before 1970 is the epoch and a time
/// after jiff's last timestamp is that timestamp.
fn clamped(time: SystemTime) -> Timestamp {
    match time.duration_since(UNIX_EPOCH) {
        Err(_) => Timestamp::UNIX_EPOCH,
        Ok(since) => Timestamp::new(
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
            i32::try_from(since.subsec_nanos()).unwrap_or(0),
        )
        .unwrap_or(Timestamp::MAX),
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
    let at: Timestamp = text.parse().ok()?;
    let seconds = u64::try_from(at.as_second()).ok();
    let nanos = u32::try_from(at.subsec_nanosecond()).ok()?;
    match seconds {
        Some(seconds) => UNIX_EPOCH.checked_add(Duration::new(seconds, nanos)),
        None => {
            // Before 1970: whole seconds back, then the fraction forward.
            let back = Duration::from_secs(at.as_second().unsigned_abs());
            UNIX_EPOCH
                .checked_sub(back)?
                .checked_add(Duration::from_nanos(nanos.into()))
        }
    }
}

#[cfg(test)]
mod tests;

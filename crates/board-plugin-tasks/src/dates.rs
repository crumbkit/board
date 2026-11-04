//! The dates a task holds: a `defer` that is a date, a moment or a horizon,
//! read as Python's `date.fromisoformat` and `datetime.fromisoformat` read
//! them. A moment is counted in seconds since 1970-01-01T00:00Z.

use board_repos::Date;

/// The named horizons of `defer`, each with its days.
pub(crate) const HORIZONS: [(&str, i64); 4] = [
    ("not-today", 1),
    ("not-this-week", 6),
    ("soon", 20),
    ("someday", 90),
];

/// The days of the horizon `name`, or none when it names none.
pub(crate) fn horizon(name: &str) -> Option<i64> {
    (HORIZONS.iter())
        .find(|(each, _)| *each == name)
        .map(|(_, days)| *days)
}

/// When a dated `defer` ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Until {
    /// The start of the person's day on this date.
    Date(Date),
    /// This moment.
    Moment(i64),
}

/// What a `defer` holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Defer {
    /// Days after `reviewed`.
    Horizon(i64),
    Until(Until),
}

impl Defer {
    /// The `defer` `text` names: a horizon, a date, or a date-time, which
    /// is a value longer than a date. A date-time without an offset is
    /// local time, `offset` seconds east of UTC. None when it is none.
    pub(crate) fn parse(text: &str, offset: i32) -> Option<Self> {
        if let Some(days) = horizon(text) {
            return Some(Self::Horizon(days));
        }
        until(text, offset).map(Self::Until)
    }
}

/// The date or moment a dated `defer` names.
pub(crate) fn until(text: &str, offset: i32) -> Option<Until> {
    if text.len() <= "2026-10-25".len() {
        return Date::parse(text).map(Until::Date);
    }
    moment(text, offset).map(Until::Moment)
}

/// The moment of a date-time such as `2026-10-25T09:00:00+03:00`: a date,
/// `T` or `t`, `HH:MM`, optional seconds and fraction, and an offset, `Z`,
/// `±HH:MM` or none for local time. A leap second is refused, as Python
/// refuses it.
fn moment(text: &str, offset: i32) -> Option<i64> {
    let date = Date::parse(text.get(..10)?)?;
    let rest = text.get(10..)?;
    let rest = rest.strip_prefix(['T', 't'])?;
    let (clock, zone) = match rest.find(['Z', 'z', '+', '-']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let seconds = clock_seconds(clock)?;
    let east = match zone {
        "" => i64::from(offset),
        "Z" | "z" => 0,
        signed => zone_seconds(signed)?,
    };
    Some(date.days() * 86_400 + seconds - east)
}

/// The seconds into the day of `HH:MM`, `HH:MM:SS` or `HH:MM:SS.fff`.
fn clock_seconds(clock: &str) -> Option<i64> {
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, "0"));
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = whole.split(':').collect();
    if !(2..=3).contains(&parts.len()) || (parts.len() == 2 && clock.contains('.')) {
        return None;
    }
    let number = |part: &str, below: i64| {
        (part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<i64>().ok())
            .flatten()
            .filter(|value| *value < below)
    };
    let hours = number(parts[0], 24)?;
    let minutes = number(parts[1], 60)?;
    let seconds = parts.get(2).map_or(Some(0), |part| number(part, 60))?;
    Some(hours * 3600 + minutes * 60 + seconds)
}

/// The seconds east of UTC of `+HH:MM`, `-HH:MM` or `+HHMM`.
fn zone_seconds(zone: &str) -> Option<i64> {
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
    let well_formed = matches!(zone[1..].len(), 4 | 5)
        && (zone[1..].len() == 4 || zone.as_bytes()[3] == b':')
        && digits.len() == 4
        && digits.bytes().all(|byte| byte.is_ascii_digit());
    if !well_formed {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok().filter(|hours| *hours < 24)?;
    let minutes: i64 = digits[2..].parse().ok().filter(|minutes| *minutes < 60)?;
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(test)]
mod tests;

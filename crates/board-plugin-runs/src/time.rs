//! A run's `started` and `finished`, read and printed as the Python plugin's
//! pydantic `AwareDatetime` did.

use std::time::{SystemTime, UNIX_EPOCH};

/// A time with its offset from UTC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Time {
    /// Microseconds since 1970-01-01T00:00:00Z, which orders times.
    pub(crate) micros: i64,
    /// The time as pydantic printed it: `T`, the fraction only when not
    /// zero and then to six digits, and `Z` for a zero offset.
    pub(crate) text: String,
}

impl Time {
    /// `YYYY-MM-DD(T|t| |_)HH:MM[:SS[(.|,)F+]](Z|z|±HH:MM|±HHMM)` in ASCII digits,
    /// a real date from year 1, hour to 23, second to 59, and an offset under
    /// 24 hours. Fraction digits past the sixth are dropped.
    ///
    /// Pydantic also read a bare number as seconds since the epoch. A
    /// shipped schema's `date-time` admits no number, so only a board with a
    /// looser schema of its own could hold one; such a run is unreadable
    /// here.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        let number = |from: usize, to: usize| -> Option<i64> {
            (bytes.get(from..to)?.iter()).try_fold(0, |number, &digit| {
                (digit.is_ascii_digit()).then(|| number * 10 + i64::from(digit - b'0'))
            })
        };
        let at =
            |index: usize, allowed: &[u8]| bytes.get(index).is_some_and(|b| allowed.contains(b));
        if !(at(4, b"-") && at(7, b"-") && at(10, b"Tt _") && at(13, b":")) {
            return None;
        }
        let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
        let (hour, minute) = (number(11, 13)?, number(14, 16)?);
        let mut rest = &bytes[16..];
        let mut second = 0;
        let mut micros = 0;
        if let Some(seconds) = rest.strip_prefix(b":") {
            second = number(17, 19)?;
            rest = &seconds[2..];
            if let Some(fraction) = rest.strip_prefix(b".").or_else(|| rest.strip_prefix(b",")) {
                let digits = fraction.iter().take_while(|byte| byte.is_ascii_digit());
                let (count, padded) = (digits.clone().count(), digits.chain(&[b'0'; 6]).take(6));
                micros = padded.fold(0, |micros, &digit| micros * 10 + i64::from(digit - b'0'));
                rest = (count > 0).then(|| &fraction[count..])?;
            }
        }
        let offset = match rest {
            b"Z" | b"z" => 0,
            [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            | [sign @ (b'+' | b'-'), h1, h2, m1, m2] => {
                let digits = [*h1, *h2, *m1, *m2];
                if !digits.iter().all(u8::is_ascii_digit) {
                    return None;
                }
                let [h1, h2, m1, m2] = digits.map(|digit| i64::from(digit - b'0'));
                let minutes = (h1 * 10 + h2) * 60 + m1 * 10 + m2;
                if minutes >= 24 * 60 || m1 * 10 + m2 > 59 {
                    return None;
                }
                if *sign == b'-' { -minutes } else { minutes }
            }
            _ => return None,
        };
        let valid = year >= 1
            && (1..=12).contains(&month)
            && (1..=days_in(year, month)).contains(&day)
            && hour <= 23
            && minute <= 59
            && second <= 59;
        if !valid {
            return None;
        }
        let seconds = days(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
        let fraction = if micros == 0 {
            String::new()
        } else {
            format!(".{micros:06}")
        };
        let zone = if offset == 0 {
            "Z".to_owned()
        } else {
            let sign = if offset < 0 { '-' } else { '+' };
            format!("{sign}{:02}:{:02}", offset.abs() / 60, offset.abs() % 60)
        };
        Some(Self {
            micros: (seconds - offset * 60) * 1_000_000 + micros,
            text: format!(
                "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{fraction}{zone}"
            ),
        })
    }

    /// The whole seconds from this time to `now`, or 0 when it is later.
    pub(crate) fn seconds_before(&self, now: SystemTime) -> u64 {
        let now = match now.duration_since(UNIX_EPOCH) {
            Ok(since) => i128::try_from(since.as_micros()).unwrap_or(i128::MAX),
            Err(before) => -i128::try_from(before.duration().as_micros()).unwrap_or(i128::MAX),
        };
        let age = (now - i128::from(self.micros)) / 1_000_000;
        u64::try_from(age).unwrap_or(0)
    }
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar, by
/// Howard Hinnant's `days_from_civil`.
fn days(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468
}

#[cfg(test)]
mod tests;

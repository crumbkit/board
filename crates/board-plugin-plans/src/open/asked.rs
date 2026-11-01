//! Reads `asked` as Python's `datetime.fromisoformat` did for the Python plugin.

/// An instant: seconds since 1970-01-01T00:00:00Z, then microseconds.
pub type Instant = (i64, u32);

/// `YYYY-MM-DDTHH:MM:SS[.F+](Z|±HH:MM)` in ASCII digits, `T` and `Z` in either
/// case, a real date from year 1, hour to 23, offset under 24 hours whose
/// minutes may run to 99. Fraction digits past the sixth are dropped.
pub fn instant(text: &str) -> Option<Instant> {
    let bytes = text.as_bytes();
    let number = |from: usize, to: usize| -> Option<i64> {
        (bytes.get(from..to)?.iter()).try_fold(0, |number, &digit| {
            digit
                .is_ascii_digit()
                .then(|| number * 10 + i64::from(digit - b'0'))
        })
    };
    let at =
        |index: usize, allowed: &[u8]| bytes.get(index).is_some_and(|byte| allowed.contains(byte));
    if !(at(4, b"-") && at(7, b"-") && at(10, b"Tt") && at(13, b":") && at(16, b":")) {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let mut rest = &bytes[19..];
    let mut micros = 0;
    if let Some(fraction) = rest.strip_prefix(b".") {
        let digits = fraction.iter().take_while(|byte| byte.is_ascii_digit());
        let (digits, padded) = (digits.clone().count(), digits.chain(&[b'0'; 6]).take(6));
        micros = padded.fold(0, |micros, &digit| micros * 10 + u32::from(digit - b'0'));
        rest = (digits > 0).then(|| &fraction[digits..])?;
    }
    let offset = match rest {
        b"Z" | b"z" => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let start = bytes.len() - 5;
            let minutes = number(start, start + 2)? * 60 + number(start + 3, start + 5)?;
            if minutes >= 24 * 60 {
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
    valid.then(|| {
        let seconds = days(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
        (seconds - offset * 60, micros)
    })
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

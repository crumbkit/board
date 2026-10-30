use proptest::prelude::*;

use super::*;

/// Each time Python's `fromisoformat` refuses, beside a twin it reads, as
/// Python 3.13 answered them.
const TWINS: [(&str, &str); 14] = [
    ("2026-10-01T09:00:00", "2026-10-01T09:00:00Z"),
    ("2026-10-01", "2026-10-01T00:00:00Z"),
    ("2026-10-01T09:00:00+0000", "2026-10-01T09:00:00+00:00"),
    ("2026-10-01T23:59:60Z", "2026-10-01T23:59:59Z"),
    ("2026-10-01T24:00:00Z", "2026-10-01T23:00:00Z"),
    ("2026-10-01T09:60:00Z", "2026-10-01T09:59:00Z"),
    ("2026-02-29T00:00:00Z", "2024-02-29T00:00:00Z"),
    ("2026-02-30T00:00:00Z", "2026-02-28T00:00:00Z"),
    ("2026-13-01T00:00:00Z", "2026-12-01T00:00:00Z"),
    ("0000-01-01T00:00:00Z", "0001-01-01T00:00:00Z"),
    ("2026-10-01T09:00:00+24:00", "2026-10-01T09:00:00+23:59"),
    ("2026-10-01T09:00:00+23:99", "2026-10-01T09:00:00+00:99"),
    ("2026-10-01T09:00:00.Z", "2026-10-01T09:00:00.0Z"),
    ("٢٠٢٦-10-01T09:00:00Z", "2026-10-01T09:00:00Z"),
];

#[test]
fn every_time_python_refuses_is_refused_and_its_twin_is_read() {
    for (refused, read) in TWINS {
        assert_eq!(instant(refused), None, "{refused}");
        assert!(instant(read).is_some(), "{read}");
    }
}

#[test]
fn a_time_is_read_as_seconds_since_the_epoch_in_utc_and_microseconds() {
    assert_eq!(instant("1970-01-01T00:00:00Z"), Some((0, 0)));
    assert_eq!(instant("1970-01-01t01:00:00+01:00"), Some((0, 0)));
    assert_eq!(instant("1970-01-01T00:00:00-00:01"), Some((60, 0)));
    assert_eq!(instant("1970-01-01T00:00:00+00:99"), Some((-99 * 60, 0)));
    assert_eq!(instant("2026-10-01T09:00:00z"), Some((1_790_845_200, 0)));
    assert_eq!(instant("2000-03-01T00:00:00Z"), Some((951_868_800, 0)));
    assert_eq!(instant("0001-01-01T00:00:00Z"), Some((-62_135_596_800, 0)));
}

#[test]
fn fraction_digits_past_the_sixth_are_dropped_and_fewer_are_padded() {
    let at = |fraction: &str| instant(&format!("1970-01-01T00:00:00.{fraction}Z"));
    assert_eq!(at("5"), Some((0, 500_000)));
    assert_eq!(at("123456"), Some((0, 123_456)));
    assert_eq!(at("123456789"), Some((0, 123_456)));
    assert_eq!(at("0000000"), Some((0, 0)));
}

proptest! {
    #[test]
    fn instants_follow_the_calendar_and_an_offset_moves_them_by_its_minutes(
        year in 1_i64..=9999,
        month in 1_i64..=12,
        day in 1_i64..=31,
        hour in 0_i64..=23,
        minute in 0_i64..=59,
        second in 0_i64..=59,
        offset in -1439_i64..1440,
    ) {
        let day = day.min(days_in(year, month));
        let text = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
        let utc = instant(&format!("{text}Z")).unwrap();
        let sign = if offset < 0 { '-' } else { '+' };
        let shifted = format!("{text}{sign}{:02}:{:02}", offset.abs() / 60, offset.abs() % 60);
        prop_assert_eq!(instant(&shifted), Some((utc.0 - offset * 60, 0)));
        let midnight = instant(&format!("{year:04}-{month:02}-{day:02}T00:00:00Z")).unwrap();
        prop_assert_eq!(utc.0 - midnight.0, hour * 3_600 + minute * 60 + second);
        prop_assert_eq!(midnight.0.rem_euclid(86_400), 0);
        let next = if day < days_in(year, month) {
            Some(format!("{year:04}-{month:02}-{:02}", day + 1))
        } else if month < 12 {
            Some(format!("{year:04}-{:02}-01", month + 1))
        } else {
            (year < 9999).then(|| format!("{:04}-01-01", year + 1))
        };
        if let Some(next) = next {
            let next = instant(&format!("{next}T00:00:00Z")).unwrap();
            prop_assert_eq!(next.0 - midnight.0, 86_400);
        }
    }
}

#[test]
fn bolero_any_text_is_read_as_an_instant_only_in_the_shape_of_one() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if let Some((_, micros)) = instant(text) {
            assert!(micros < 1_000_000, "{text:?}");
            assert!(text.is_ascii() && text.len() >= 20, "{text:?}");
            assert!(matches!(text.as_bytes()[10], b'T' | b't'), "{text:?}");
        }
    });
}

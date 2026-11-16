//! Days read, print and subtract as the calendar says, across leap years.

use std::time::Duration;

use super::*;

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn a_day_prints_as_it_was_read_and_the_epoch_is_day_zero() {
    for text in [
        "1970-01-01",
        "2000-02-29",
        "2026-11-15",
        "2100-03-01",
        "1969-12-31",
    ] {
        assert_eq!(day(text).to_string(), text);
    }
    assert_eq!(day("1970-01-01"), Date(0));
    assert_eq!(day("2026-11-15"), Date(20_772));
}

#[test]
fn days_since_counts_across_months_and_leap_days() {
    assert_eq!(day("2026-11-15").days_since(day("2026-11-11")), 4);
    assert_eq!(day("2024-03-01").days_since(day("2024-02-28")), 2);
    assert_eq!(day("2025-03-01").days_since(day("2025-02-28")), 1);
    assert_eq!(day("2100-03-01").days_since(day("2100-02-28")), 1);
    assert_eq!(day("2026-01-01").days_since(day("2027-01-01")), -365);
}

#[test]
fn text_that_is_not_a_calendar_day_is_refused() {
    for text in [
        "",
        "2026-1-15",
        "2026-13-01",
        "2026-02-29",
        "2026-04-31",
        "2026-11-00",
        "2026/11/15",
        "2026-11-15T00:00",
        "+026-11-15",
        "2026-11-1a",
    ] {
        assert!(text.parse::<Date>().is_err(), "{text:?}");
    }
    assert!("2024-02-29".parse::<Date>().is_ok());
}

#[test]
fn the_clock_gives_the_utc_day_and_a_stamp() {
    let at = UNIX_EPOCH + Duration::from_secs(1_794_000_000 + 3_661);
    let (date, _) = utc(at);
    assert_eq!(stamp(at).len(), 16);
    assert_eq!(stamp(UNIX_EPOCH), "19700101T000000Z");
    assert_eq!(
        utc(UNIX_EPOCH + Duration::from_secs(86_399)),
        (Date(0), 86_399)
    );
    assert_eq!(utc(UNIX_EPOCH + Duration::from_hours(24)), (Date(1), 0));
    assert_eq!(
        stamp(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        "20231114T221320Z"
    );
    assert_eq!(date.to_string().replace('-', ""), stamp(at)[..8]);
}

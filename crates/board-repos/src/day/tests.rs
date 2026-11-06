//! The person's day starts at `day_start`, or at midnight. These were the
//! former Python library's `test_day.py`.

use std::time::{Duration, SystemTime};

use super::*;

const SEVEN: Duration = Duration::from_hours(7);
/// London's summer time, an hour east of UTC.
const LONDON: i32 = 3600;

/// The moment that is `hour`:`minute` on 2026-10-25 an hour east of UTC.
fn at(hour: u64, minute: u64) -> SystemTime {
    // 2026-10-25T00:00Z is day 20,751 after 1970-01-01.
    let midnight = SystemTime::UNIX_EPOCH + Duration::from_hours(20_751 * 24);
    midnight + Duration::from_hours(hour) + Duration::from_mins(minute) - Duration::from_hours(1)
}

#[test]
fn a_moment_before_the_day_starts_belongs_to_yesterday_and_one_at_the_start_to_today() {
    assert_eq!(today(at(1, 0), LONDON, SEVEN).to_string(), "2026-10-24");
    assert_eq!(today(at(6, 59), LONDON, SEVEN).to_string(), "2026-10-24");
    assert_eq!(today(at(7, 0), LONDON, SEVEN).to_string(), "2026-10-25");
    assert_eq!(
        today(at(0, 0), LONDON, Duration::ZERO).to_string(),
        "2026-10-25"
    );
}

#[test]
fn the_day_starts_at_day_start_and_at_midnight_without_it() {
    assert_eq!(day_start(None), Duration::ZERO);
    assert_eq!(
        day_start(Some("07:30")),
        Duration::from_hours(7) + Duration::from_mins(30)
    );
}

#[test]
fn a_date_is_shown_as_iso_8601_across_a_leap_day_and_before_1970() {
    let day = |days: i64| Date::from_days(days).to_string();
    assert_eq!(day(0), "1970-01-01");
    assert_eq!(day(-1), "1969-12-31");
    assert_eq!(day(19_416), "2023-02-28");
    assert_eq!(day(19_782), "2024-02-29");
    assert_eq!(day(19_783), "2024-03-01");
}

#[test]
fn a_date_reads_only_as_yyyy_mm_dd_naming_a_day_its_month_has() {
    let read = |text: &str| Date::parse(text).map(|date| date.to_string());
    assert_eq!(read("2028-02-29").as_deref(), Some("2028-02-29"));
    for refused in [
        "2027-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-1-01",
        "20261025",
        "2026-10-25T",
        "0000-01-01",
        "2026-10-3a",
    ] {
        assert_eq!(read(refused), None, "{refused}");
    }
}

#[test]
fn days_count_from_1970_both_ways_and_weekdays_start_on_monday() {
    for days in [-800_000, -1, 0, 19_416, 19_782, 20_751, 800_000] {
        assert_eq!(Date::from_days(days).days(), days);
    }
    let monday = Date::parse("2026-10-26").unwrap();
    assert_eq!((monday.weekday(), monday.plus(6).weekday()), (0, 6));
    assert_eq!(monday.plus(-26).to_string(), "2026-09-30");
    assert_eq!(Date::clamped(2027, 2, 31).to_string(), "2027-02-28");
    assert_eq!(Date::clamped(2028, 2, 31).to_string(), "2028-02-29");
}

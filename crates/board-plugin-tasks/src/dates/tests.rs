//! A `defer` reads as a horizon, a date or a moment, and refuses what
//! Python's readers refuse.

use super::*;

/// 2026-10-25T00:00Z.
const MIDNIGHT: i64 = 20_751 * 86_400;

#[test]
fn a_date_time_reads_with_its_offset_and_without_one_as_local_time() {
    let read = |text: &str| moment(text, 3 * 3600);
    assert_eq!(read("2026-10-25T09:00:00+03:00"), Some(MIDNIGHT + 6 * 3600));
    assert_eq!(read("2026-10-25t09:00:00Z"), Some(MIDNIGHT + 9 * 3600));
    assert_eq!(read("2026-10-25T09:00:00.25z"), Some(MIDNIGHT + 9 * 3600));
    assert_eq!(read("2026-10-25T09:00"), Some(MIDNIGHT + 6 * 3600));
    assert_eq!(
        read("2026-10-25T09:00:00-0130"),
        Some(MIDNIGHT + 10 * 3600 + 1800)
    );
}

#[test]
fn a_date_time_python_refuses_is_refused_and_its_twin_is_read() {
    for (refused, admitted) in [
        ("2026-06-30T23:59:60Z", "2026-06-30T23:59:59Z"),
        ("2026-10-25T25:00:00Z", "2026-10-25T23:00:00Z"),
        ("2026-10-25 10:00:00Z", "2026-10-25T10:00:00Z"),
        ("2026-10-25T10:00:00+25:00", "2026-10-25T10:00:00+05:00"),
        ("2026-02-30T10:00:00Z", "2026-02-28T10:00:00Z"),
        ("2026-10-25T1:00:00Z", "2026-10-25T01:00:00Z"),
    ] {
        assert_eq!(moment(refused, 0), None, "{refused}");
        assert!(moment(admitted, 0).is_some(), "{admitted}");
    }
}

#[test]
fn a_defer_is_a_horizon_a_date_or_a_moment_by_its_length() {
    assert_eq!(Defer::parse("soon", 0), Some(Defer::Horizon(20)));
    assert_eq!(
        Defer::parse("2026-10-25", 0),
        Some(Defer::Until(Until::Date(
            Date::parse("2026-10-25").unwrap()
        )))
    );
    assert_eq!(
        Defer::parse("2026-10-25T00:00Z", 0),
        Some(Defer::Until(Until::Moment(MIDNIGHT)))
    );
    assert_eq!(Defer::parse("tomorrow", 0), None);
    assert_eq!(Defer::parse("next-year", 0), None);
}

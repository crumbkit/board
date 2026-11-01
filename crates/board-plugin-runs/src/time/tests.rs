//! A time reads and prints as pydantic's `AwareDatetime` did, and orders by
//! its instant whatever its offset.

use std::time::Duration;

use super::*;

#[test]
fn a_time_prints_as_pydantic_printed_it_and_a_time_pydantic_refused_is_not_read() {
    for (given, printed) in [
        ("2026-10-02T09:00:00Z", Some("2026-10-02T09:00:00Z")),
        ("2026-10-02T09:00:00+00:00", Some("2026-10-02T09:00:00Z")),
        ("2026-10-02T09:00:00-00:00", Some("2026-10-02T09:00:00Z")),
        (
            "2026-10-02T09:00:00.5+02:00",
            Some("2026-10-02T09:00:00.500000+02:00"),
        ),
        ("2026-10-02T09:00:00.000Z", Some("2026-10-02T09:00:00Z")),
        (
            "2026-10-02T09:00:00.123456789Z",
            Some("2026-10-02T09:00:00.123456Z"),
        ),
        ("2026-10-02 09:00:00z", Some("2026-10-02T09:00:00Z")),
        ("2026-10-02t09:00:00Z", Some("2026-10-02T09:00:00Z")),
        ("2026-10-02T09:00Z", Some("2026-10-02T09:00:00Z")),
        (
            "2026-10-02T09:00:00+0200",
            Some("2026-10-02T09:00:00+02:00"),
        ),
        (
            "2026-10-02T09:00:00-05:30",
            Some("2026-10-02T09:00:00-05:30"),
        ),
        (
            "2026-10-02T09:00:00+23:59",
            Some("2026-10-02T09:00:00+23:59"),
        ),
        ("2024-02-29T00:00:00Z", Some("2024-02-29T00:00:00Z")),
        ("0001-01-01T00:00:00Z", Some("0001-01-01T00:00:00Z")),
        ("2026-10-02T09:00:00", None),
        ("2026-10-02", None),
        ("2026-10-02T24:00:00Z", None),
        ("2026-10-02T09:00:60Z", None),
        ("2026-10-02T09:00:00+24:00", None),
        ("2026-10-02T09:00:00+02:60", None),
        ("2026-10-02T09:00:00+02", None),
        ("2026-10-02T09:00:00.Z", None),
        ("2026-02-29T00:00:00Z", None),
        ("0000-01-01T00:00:00Z", None),
    ] {
        let read = Time::parse(given);
        assert_eq!(
            read.as_ref().map(|time| time.text.as_str()),
            printed,
            "{given}"
        );
    }
}

#[test]
fn times_order_by_their_instant_whatever_their_offset() {
    let at = |text: &str| Time::parse(text).unwrap().micros;
    assert_eq!(at("2026-10-02T11:00:00+02:00"), at("2026-10-02T09:00:00Z"));
    assert!(at("2026-10-02T10:00:00+02:00") < at("2026-10-02T09:00:00Z"));
    assert!(at("2026-10-02T09:00:00Z") < at("2026-10-02T09:00:00.000001Z"));
    assert_eq!(at("1970-01-01T00:00:01Z"), 1_000_000);
}

#[test]
fn the_seconds_before_now_are_whole_and_never_below_zero() {
    let time = Time::parse("1970-01-01T00:00:10.5Z").unwrap();
    let now = |micros: u64| UNIX_EPOCH + Duration::from_micros(micros);
    assert_eq!(time.seconds_before(now(20_499_999)), 9);
    assert_eq!(time.seconds_before(now(20_500_000)), 10);
    assert_eq!(time.seconds_before(now(10_000_000)), 0);
    assert_eq!(time.seconds_before(now(0)), 0);
}

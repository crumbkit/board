use std::time::{Duration, UNIX_EPOCH};

use super::*;

#[test]
fn a_utc_time_is_rfc_3339_to_the_microsecond_with_a_fixed_width_of_27() {
    let at = |micros| utc(UNIX_EPOCH + Duration::from_micros(micros));
    assert_eq!(at(0), "1970-01-01T00:00:00.000000Z");
    let leap = at(1_709_251_199_000_000);
    assert_eq!(leap, "2024-02-29T23:59:59.000000Z");
    let now = at(1_793_400_294_102_864);
    assert_eq!(now, "2026-10-30T22:44:54.102864Z");
    assert_eq!((leap.len(), now.len()), (27, 27));
    assert!(leap < now);
}

#[test]
fn a_utc_seconds_time_is_rfc_3339_to_the_second() {
    let at = |seconds| utc_seconds(UNIX_EPOCH + Duration::from_secs(seconds));
    assert_eq!(at(0), "1970-01-01T00:00:00Z");
    assert_eq!(at(1_709_251_199), "2024-02-29T23:59:59Z");
    assert_eq!(at(1_793_400_294), "2026-10-30T22:44:54Z");
    assert_eq!(
        utc_seconds(UNIX_EPOCH + Duration::from_micros(1_793_400_294_999_999)),
        "2026-10-30T22:44:54Z"
    );
}

#[test]
fn a_time_before_1970_or_after_the_last_timestamp_is_clamped() {
    let before = UNIX_EPOCH - Duration::from_secs(1);
    assert_eq!(utc(before), "1970-01-01T00:00:00.000000Z");
    assert_eq!(utc_seconds(before), "1970-01-01T00:00:00Z");
    let after = UNIX_EPOCH + Duration::from_secs(300_000_000_000);
    assert_eq!(utc(after), "9999-12-30T22:00:00.999999Z");
    assert_eq!(utc_seconds(after), "9999-12-30T22:00:00Z");
}

#[test]
fn a_moment_reads_z_and_any_offset_as_the_same_instant() {
    let utc = moment("2026-11-08T09:00:00Z").unwrap();
    assert_eq!(moment("2026-11-08T10:00:00+01:00"), Some(utc));
    assert_eq!(moment("2026-11-08T03:30:00-05:30"), Some(utc));
    assert_eq!(moment("2026-11-08T09:00:00+00:00"), Some(utc));
    assert_eq!(moment("2026-11-08t09:00:00z"), Some(utc));
    assert_eq!(
        moment("2026-11-08T09:00:00.500Z"),
        Some(utc + Duration::from_millis(500))
    );
}

#[test]
fn a_moment_refuses_what_is_no_time_and_a_fraction_of_ten_digits() {
    assert_eq!(moment("not a time"), None);
    assert_eq!(moment("2026-11-08T09:00:00+xx:00"), None);
    assert_eq!(moment("2026-11-08T09:00:00.1234567890Z"), None);
}

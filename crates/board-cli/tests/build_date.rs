//! `BOARD_BUILD_DATE`, the commit date a build embeds, comes from
//! `SOURCE_DATE_EPOCH` when it is set, and otherwise from git's HEAD; with
//! neither, a build has no date (docs/architecture/writes/minimum.md).

#[path = "../build/built.rs"]
mod built;
#[path = "../../board-ops/src/version/civil.rs"]
mod civil;

use std::ffi::OsStr;

use built::seconds;
use civil::{parse_utc, utc};

fn os(value: &str) -> &OsStr {
    OsStr::new(value)
}

fn git(text: &str) -> impl FnOnce(Option<&str>) -> Option<String> {
    let text = text.to_owned();
    move |_| Some(text)
}

fn no_git(_: Option<&str>) -> Option<String> {
    None
}

#[test]
fn source_date_epoch_wins_over_git() {
    assert_eq!(
        seconds(Some(os("1794528000")), None, git("5")),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn git_dates_the_build_when_source_date_epoch_is_unset_or_empty() {
    assert_eq!(
        seconds(None, None, git("1794528000\n")),
        Ok(Some(1_794_528_000))
    );
    assert_eq!(
        seconds(Some(os("")), None, git("1794528000")),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn a_build_with_neither_has_no_date() {
    assert_eq!(seconds(None, None, no_git), Ok(None));
    assert_eq!(seconds(None, None, git("not a number")), Ok(None));
}

#[test]
fn a_source_date_epoch_that_is_not_whole_seconds_fails_the_build() {
    let message = seconds(Some(os("yesterday")), None, git("5")).unwrap_err();
    assert!(
        message.contains("SOURCE_DATE_EPOCH is \"yesterday\""),
        "{message}"
    );
}

#[test]
fn seconds_print_as_a_utc_timestamp() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(1_794_528_000), "2026-11-13T00:00:00Z");
    assert_eq!(utc(1_709_210_096), "2024-02-29T12:34:56Z");
    assert_eq!(utc(-1), "1969-12-31T23:59:59Z");
}

/// A `git log` that knows these commits and their dates.
fn log(commit: Option<&str>) -> Option<String> {
    match commit {
        None => Some("100".to_owned()),
        Some("abc123") => Some("1794528000\n".to_owned()),
        Some(_) => None,
    }
}

#[test]
fn the_named_build_commit_dates_the_build_in_place_of_head() {
    assert_eq!(
        seconds(None, Some(os("abc123")), log),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn a_build_commit_git_cannot_read_leaves_the_build_without_a_date() {
    assert_eq!(seconds(None, Some(os("feedface")), log), Ok(None));
}

#[test]
fn an_empty_build_commit_is_unset_and_source_date_epoch_still_wins() {
    assert_eq!(seconds(None, Some(os("")), log), Ok(Some(100)));
    assert_eq!(seconds(Some(os("7")), Some(os("abc123")), log), Ok(Some(7)));
}

#[test]
fn a_timestamp_with_a_zone_reads_back_to_the_seconds_it_prints() {
    assert_eq!(parse_utc("2026-11-13T00:00:00Z"), Some(1_794_528_000));
    assert_eq!(parse_utc("2026-11-13T10:00:00+10:00"), Some(1_794_528_000));
    assert_eq!(
        parse_utc("2026-11-12T19:30:00.25-04:30"),
        Some(1_794_528_000)
    );
    assert_eq!(parse_utc("1969-12-31T23:59:59Z"), Some(-1));
}

#[test]
fn a_leap_day_exists_in_a_leap_year_only() {
    assert_eq!(parse_utc("2024-02-29T12:34:56Z"), Some(1_709_210_096));
    assert_eq!(utc(1_709_210_096), "2024-02-29T12:34:56Z");
    assert_eq!(parse_utc("2000-02-29T00:00:00Z"), Some(951_782_400));
    assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(parse_utc("2023-02-29T00:00:00Z"), None);
    assert_eq!(parse_utc("2100-02-29T00:00:00Z"), None);
}

#[test]
fn text_that_is_not_a_timestamp_with_a_zone_is_not_read() {
    for bad in [
        "",
        "2026-11-13",
        "2026-11-13T10:00:00",
        "2026-13-01T00:00:00Z",
        "2026-11-31T00:00:00Z",
        "2026-11-13T24:00:00Z",
        "2026-11-13T10:00:00.Z",
        "2026-11-13T10:00:00+1000",
        "yesterday",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad}");
    }
}

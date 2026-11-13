//! `BOARD_BUILD_DATE`, the commit date a build embeds, comes from
//! `SOURCE_DATE_EPOCH` when it is set, and otherwise from git's HEAD; with
//! neither, a build has no date (docs/architecture/writes/minimum.md).

#[path = "../build/built.rs"]
mod built;

use std::ffi::OsStr;

use built::{seconds, utc};

fn os(value: &str) -> &OsStr {
    OsStr::new(value)
}

fn git(text: &str) -> impl FnOnce() -> Option<String> {
    let text = text.to_owned();
    move || Some(text)
}

fn no_git() -> Option<String> {
    None
}

#[test]
fn source_date_epoch_wins_over_git() {
    assert_eq!(
        seconds(Some(os("1794528000")), git("5")),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn git_dates_the_build_when_source_date_epoch_is_unset_or_empty() {
    assert_eq!(seconds(None, git("1794528000\n")), Ok(Some(1_794_528_000)));
    assert_eq!(
        seconds(Some(os("")), git("1794528000")),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn a_build_with_neither_has_no_date() {
    assert_eq!(seconds(None, no_git), Ok(None));
    assert_eq!(seconds(None, git("not a number")), Ok(None));
}

#[test]
fn a_source_date_epoch_that_is_not_whole_seconds_fails_the_build() {
    let message = seconds(Some(os("yesterday")), git("5")).unwrap_err();
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

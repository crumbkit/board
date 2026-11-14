//! `BOARD_BUILD_DATE`, the commit date a build embeds, comes from git's
//! commit date, and from `SOURCE_DATE_EPOCH` only when git gives none; with
//! neither, a build has no date (docs/architecture/writes/minimum.md).

#[path = "../build/built.rs"]
mod built;

use std::ffi::OsStr;

use built::seconds;

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
fn the_commit_date_wins_over_source_date_epoch() {
    // Nix exports 1980 for every build, and a packager sets its own date.
    assert_eq!(
        seconds(Some(os("315532800")), None, git("1794528000")),
        Ok(Some(1_794_528_000))
    );
}

#[test]
fn source_date_epoch_dates_a_build_whose_commit_git_cannot_read() {
    assert_eq!(
        seconds(Some(os("1794528000")), None, no_git),
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
fn a_source_date_epoch_that_is_not_whole_seconds_fails_the_build_when_git_gives_no_date() {
    assert_eq!(
        seconds(Some(os("yesterday")), None, git("5")),
        Ok(Some(5)),
        "git's date is used, so the epoch is never read"
    );
    let message = seconds(Some(os("yesterday")), None, no_git).unwrap_err();
    assert!(
        message.contains("SOURCE_DATE_EPOCH is \"yesterday\""),
        "{message}"
    );
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
fn an_empty_build_commit_is_unset_and_a_commit_git_cannot_read_falls_back_to_source_date_epoch() {
    assert_eq!(seconds(None, Some(os("")), log), Ok(Some(100)));
    assert_eq!(
        seconds(Some(os("7")), Some(os("abc123")), log),
        Ok(Some(1_794_528_000))
    );
    assert_eq!(
        seconds(Some(os("7")), Some(os("feedface")), log),
        Ok(Some(7))
    );
}

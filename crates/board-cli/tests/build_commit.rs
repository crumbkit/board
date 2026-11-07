//! `BOARD_BUILD`, the suffix after Board's version, names the commit that
//! `BOARD_BUILD_COMMIT` gives when it is set, and otherwise the commit git
//! names; either way the local git says whether the source differs from it
//! (docs/architecture/performance/timing.md).

#[path = "../build/commit.rs"]
mod commit;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use commit::suffix;
use proptest::prelude::*;

const FULL: &str = "45aafa9e1b2c3d4e5f60718293a4b5c6d7e8f901";

fn os(value: &str) -> &OsStr {
    OsStr::new(value)
}

/// Git names no commit.
fn no_git() -> impl FnOnce() -> Option<String> {
    || None
}

/// The local git says the source does not differ.
fn clean() -> impl FnOnce() -> Option<String> {
    || None
}

/// The local git says the source differs, with this hash.
fn dirty() -> impl FnOnce() -> Option<String> {
    || Some("3f0c2a9b8d7e6f15".to_owned())
}

fn refusal(value: &str) -> String {
    format!(
        "BOARD_BUILD_COMMIT is \"{value}\", which is not a commit; set it to the 7 to 40 hex digits of the commit the source was taken from, or unset it"
    )
}

#[test]
fn a_given_forty_digit_commit_names_its_first_twelve_digits_and_never_asks_git_for_its_commit() {
    let asked = || -> Option<String> { panic!("git was asked for its commit") };
    assert_eq!(
        suffix(Some(os(FULL)), asked, clean()),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_seven_digit_commit_names_all_seven() {
    assert_eq!(
        suffix(Some(os("45aafa9")), no_git(), clean()),
        Ok("+45aafa9".to_owned())
    );
}

#[test]
fn a_given_commit_in_uppercase_is_lowercased() {
    assert_eq!(
        suffix(Some(os("45AAFA9E1B2C3D")), no_git(), clean()),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_commit_names_the_changes_when_the_local_git_says_the_source_differs() {
    let head = || Some("000000000000".to_owned());
    assert_eq!(
        suffix(Some(os(FULL)), head, dirty()),
        Ok("+45aafa9e1b2c.dirty.3f0c2a9b8d7e6f15".to_owned())
    );
}

#[test]
fn a_given_commit_names_no_changes_when_the_local_git_says_the_source_is_clean() {
    let head = || Some("000000000000".to_owned());
    assert_eq!(
        suffix(Some(os(FULL)), head, clean()),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_value_with_a_character_that_is_not_hex_fails_and_its_hex_twin_passes() {
    assert_eq!(
        suffix(Some(os("45aafa9g")), no_git(), clean()),
        Err(refusal("45aafa9g"))
    );
    assert_eq!(
        suffix(Some(os("45aafa9f")), no_git(), clean()),
        Ok("+45aafa9f".to_owned())
    );
}

#[test]
fn a_given_value_of_six_digits_fails_and_seven_pass() {
    assert_eq!(
        suffix(Some(os("45aafa")), no_git(), clean()),
        Err(refusal("45aafa"))
    );
    assert_eq!(
        suffix(Some(os("45aafa9")), no_git(), clean()),
        Ok("+45aafa9".to_owned())
    );
}

#[test]
fn a_given_value_of_forty_one_digits_fails_and_forty_pass() {
    let long = format!("{FULL}0");
    assert_eq!(
        suffix(Some(os(&long)), no_git(), clean()),
        Err(refusal(&long))
    );
    assert_eq!(
        suffix(Some(os(FULL)), no_git(), clean()),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_value_that_is_not_unicode_fails_and_its_unicode_twin_passes() {
    let bytes = OsStr::from_bytes(b"45aafa9\xff");
    assert_eq!(
        suffix(Some(bytes), no_git(), clean()),
        Err("BOARD_BUILD_COMMIT is \"45aafa9\u{fffd}\", which is not Unicode, so it is not a commit; set it to the 7 to 40 hex digits of the commit the source was taken from, or unset it".to_owned())
    );
    assert_eq!(
        suffix(Some(OsStr::from_bytes(b"45aafa9f")), no_git(), clean()),
        Ok("+45aafa9f".to_owned())
    );
}

#[test]
fn a_value_with_a_newline_fails_with_a_message_on_one_line_and_its_twin_without_one_passes() {
    assert_eq!(
        suffix(Some(os("45aafa9\nf\t")), no_git(), clean()),
        Err(refusal("45aafa9\\nf\\t"))
    );
    assert_eq!(
        suffix(Some(os("45aafa9f")), no_git(), clean()),
        Ok("+45aafa9f".to_owned())
    );
}

#[test]
fn a_given_commit_with_no_local_git_names_it_as_clean() {
    let asked = || -> Option<String> { panic!("git was asked for its commit") };
    assert_eq!(
        suffix(Some(os(FULL)), asked, no_git()),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn an_unset_or_empty_variable_names_the_commit_git_names() {
    let head = || Some("45aafa9e1b2c".to_owned());
    for value in [None, Some(os(""))] {
        assert_eq!(suffix(value, head, clean()), Ok("+45aafa9e1b2c".to_owned()));
    }
}

#[test]
fn an_unset_variable_names_the_changes_when_the_source_differs_from_git_s_commit() {
    let head = || Some("45aafa9e1b2c".to_owned());
    assert_eq!(
        suffix(None, head, dirty()),
        Ok("+45aafa9e1b2c.dirty.3f0c2a9b8d7e6f15".to_owned())
    );
}

#[test]
fn an_unset_variable_outside_a_git_checkout_gives_no_suffix() {
    assert_eq!(suffix(None, no_git(), clean()), Ok(String::new()));
    assert_eq!(suffix(Some(os("")), no_git(), clean()), Ok(String::new()));
}

proptest! {
    // proptest keeps regressions beside a crate's lib.rs or main.rs, which an
    // integration test does not have; a failure prints its minimal input.
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn any_seven_to_forty_hex_digits_name_their_first_twelve_lowercased(
        value in "[0-9a-fA-F]{7,40}"
    ) {
        let expected: String = value.chars().take(12).collect::<String>().to_ascii_lowercase();
        prop_assert_eq!(suffix(Some(os(&value)), no_git(), clean()), Ok(format!("+{expected}")));
    }

    #[test]
    fn any_value_with_a_character_that_is_not_hex_fails(
        before in "[0-9a-f]{0,20}",
        bad in "[^0-9a-fA-F\\p{Cc}]",
        after in "[0-9a-f]{0,19}",
    ) {
        let value = format!("{before}{bad}{after}");
        prop_assert_eq!(suffix(Some(os(&value)), no_git(), clean()), Err(refusal(&value)));
    }
}

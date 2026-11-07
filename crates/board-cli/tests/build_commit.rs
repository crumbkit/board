//! `BOARD_BUILD`, the suffix after Board's version, names the commit that
//! `BOARD_BUILD_COMMIT` gives when it is set, and otherwise the commit git
//! names (docs/architecture/performance/timing.md).

#[path = "../build/commit.rs"]
mod commit;

use commit::suffix;
use proptest::prelude::*;

const FULL: &str = "45aafa9e1b2c3d4e5f60718293a4b5c6d7e8f901";

fn no_git() -> Option<String> {
    None
}

fn refusal(value: &str) -> String {
    format!(
        "BOARD_BUILD_COMMIT is {value:?}, which is not a commit; set it to the 7 to 40 hex digits of the commit the source was taken from, or unset it"
    )
}

#[test]
fn a_given_forty_digit_commit_names_its_first_twelve_digits_and_never_asks_git() {
    let asked = || -> Option<String> { panic!("git was asked") };
    assert_eq!(
        suffix(Some(FULL), asked, asked),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_seven_digit_commit_names_all_seven() {
    assert_eq!(
        suffix(Some("45aafa9"), no_git, no_git),
        Ok("+45aafa9".to_owned())
    );
}

#[test]
fn a_given_commit_in_uppercase_is_lowercased() {
    assert_eq!(
        suffix(Some("45AAFA9E1B2C3D"), no_git, no_git),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_commit_names_no_changes_when_the_source_differs() {
    let dirty = || Some("3f0c2a9b8d7e6f15".to_owned());
    let head = || Some("000000000000".to_owned());
    assert_eq!(
        suffix(Some(FULL), head, dirty),
        Ok("+45aafa9e1b2c".to_owned())
    );
}

#[test]
fn a_given_value_with_a_character_that_is_not_hex_fails_and_its_hex_twin_passes() {
    assert_eq!(
        suffix(Some("45aafa9g"), no_git, no_git),
        Err(refusal("45aafa9g"))
    );
    assert_eq!(
        suffix(Some("45aafa9f"), no_git, no_git),
        Ok("+45aafa9f".to_owned())
    );
}

#[test]
fn a_given_value_of_six_digits_fails_and_seven_pass() {
    assert_eq!(
        suffix(Some("45aafa"), no_git, no_git),
        Err(refusal("45aafa"))
    );
    assert!(suffix(Some("45aafa9"), no_git, no_git).is_ok());
}

#[test]
fn a_given_value_of_forty_one_digits_fails_and_forty_pass() {
    let long = format!("{FULL}0");
    assert_eq!(suffix(Some(&long), no_git, no_git), Err(refusal(&long)));
    assert!(suffix(Some(FULL), no_git, no_git).is_ok());
}

#[test]
fn an_unset_or_empty_variable_names_the_commit_git_names() {
    let head = || Some("45aafa9e1b2c".to_owned());
    for given in [None, Some("")] {
        assert_eq!(suffix(given, head, no_git), Ok("+45aafa9e1b2c".to_owned()));
    }
}

#[test]
fn an_unset_variable_names_the_changes_when_the_source_differs_from_git_s_commit() {
    let head = || Some("45aafa9e1b2c".to_owned());
    let dirty = || Some("3f0c2a9b8d7e6f15".to_owned());
    assert_eq!(
        suffix(None, head, dirty),
        Ok("+45aafa9e1b2c.dirty.3f0c2a9b8d7e6f15".to_owned())
    );
}

#[test]
fn an_unset_variable_outside_a_git_checkout_gives_no_suffix() {
    assert_eq!(suffix(None, no_git, no_git), Ok(String::new()));
    assert_eq!(suffix(Some(""), no_git, no_git), Ok(String::new()));
}

proptest! {
    // proptest keeps regressions beside a crate's lib.rs or main.rs, which an
    // integration test does not have; a failure prints its minimal input.
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn any_seven_to_forty_hex_digits_name_their_first_twelve_lowercased(
        given in "[0-9a-fA-F]{7,40}"
    ) {
        let expected: String = given.chars().take(12).collect::<String>().to_ascii_lowercase();
        prop_assert_eq!(suffix(Some(&given), no_git, no_git), Ok(format!("+{expected}")));
    }

    #[test]
    fn any_value_with_a_character_that_is_not_hex_fails(
        before in "[0-9a-f]{0,20}",
        bad in "[^0-9a-fA-F]",
        after in "[0-9a-f]{0,19}",
    ) {
        let given = format!("{before}{bad}{after}");
        prop_assert_eq!(suffix(Some(&given), no_git, no_git), Err(refusal(&given)));
    }
}

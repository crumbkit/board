//! How a requirement becomes a one-line summary.

use super::*;

#[test]
fn a_summary_is_the_first_sentence_outside_inline_code_cut_to_25_words() {
    assert_eq!(
        summary("Give each meaning one type. Prefer a library type."),
        "Give each meaning one type."
    );
    assert_eq!(summary("Use `a. B` here. Then more."), "Use `a. B` here.");
    let long = vec!["word"; 30].join(" ") + ".";
    let cut = summary(&long);
    assert_eq!(cut.split(' ').count(), 25);
    assert!(cut.ends_with("word…"), "{cut}");
    assert_eq!(summary("Two\nlines  spaced."), "Two lines spaced.");
}

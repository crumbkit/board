//! How rule tables are read, how a rule becomes a summary, and how a
//! pattern relative to a repository becomes the GLOBs a control holds.

use super::*;

const PROFILE: &str = "# base\n\n## lint\n\n| ID | Rule | Check |\n| --- | --- | --- |\n\
    | BASE-1 | Lint. | `x` |\n\n## review\n\n\
    | ID | Rule | Where it applies |\n| --- | --- | --- |\n\
    | BASE-31 | Give each meaning one type. Prefer a library type. | `**/*.rs` |\n\
    | BASE-49 | Name a closure's parameter `\\|e\\| e.code()`. | `**/*.rs`, `**/tests/**` |\n\
    \nText after the table.\n";

#[test]
fn only_rows_of_tables_with_a_where_it_applies_column_are_rules_and_escaped_pipes_are_read() {
    let rows = rules("base.md", PROFILE).unwrap();
    let read: Vec<(&str, &str, Vec<&str>)> = (rows.iter())
        .map(|row| {
            let patterns = row.patterns.iter().map(String::as_str).collect();
            (row.id.as_str(), row.rule.as_str(), patterns)
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "BASE-31",
                "Give each meaning one type. Prefer a library type.",
                vec!["**/*.rs"]
            ),
            (
                "BASE-49",
                "Name a closure's parameter `|e| e.code()`.",
                vec!["**/*.rs", "**/tests/**"]
            ),
        ]
    );
}

#[test]
fn a_rule_without_a_pattern_is_refused_and_one_with_a_pattern_is_read() {
    let table = "| ID | Rule | Where it applies |\n| --- | --- | --- |\n";
    let refused = rules("base.md", &format!("{table}| BASE-31 | A rule. |  |\n")).unwrap_err();
    assert_eq!(refused.code, "controls::standards::no_pattern");
    assert_eq!(refused.fields["rule"], json!("BASE-31"));
    let admitted = rules("base.md", &format!("{table}| BASE-31 | A rule. | `**` |\n"));
    assert_eq!(admitted.unwrap().len(), 1);
}

#[test]
fn a_summary_is_the_first_sentence_cut_to_25_words() {
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

#[test]
fn a_pattern_is_anchored_at_the_repository_and_each_double_star_slash_may_match_no_directory() {
    assert_eq!(globs("repos/inbox", "**"), ["repos/inbox/**"]);
    assert_eq!(
        globs("repos/inbox", "**/*.rs"),
        ["repos/inbox/*.rs", "repos/inbox/*/*.rs"]
    );
    assert_eq!(
        globs("repos/inbox", "crates/**/tests/**"),
        [
            "repos/inbox/crates/tests/**",
            "repos/inbox/crates/*/tests/**"
        ]
    );
    assert_eq!(globs("", "Dockerfile"), ["Dockerfile"]);
}

//! The pure parts of the pull request body: sections, a run's lines and the
//! quoting. The capability runs through the binary in board-cli's changes
//! tests.

use serde_json::{Map, Value, json};

use super::{quote, quoted, run_lines};
use crate::gates::sections;

const HEAD: &str = "a3f9c0de1234567890abcdef1234567890abcdef";

fn run(fields: &Value) -> Map<String, Value> {
    let mut body = json!({
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": false,
        "versions": {"x": HEAD},
        "results": [{"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "pass"}],
    });
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    body.as_object().unwrap().clone()
}

#[test]
fn a_section_holds_its_subsections_and_a_hash_line_in_a_fence_is_not_a_heading() {
    let plan = "# T\n\n## Outcome\n\nStops.\n\n### Details\n\n```sh\n# comment\n```\n\n## Problem\n\nHangs.\n";
    let found = sections(plan);
    assert_eq!(
        found["outcome"],
        "Stops.\n\n### Details\n\n```sh\n# comment\n```"
    );
    assert_eq!(found["details"], "```sh\n# comment\n```");
    assert_eq!(found["problem"], "Hangs.");
    assert!(!found.contains_key("comment"));
}

#[test]
fn a_run_names_cases_up_to_the_cap_and_counts_the_rest() {
    let failing = |count: usize| {
        let results: Vec<Value> = (0..count)
            .map(|number| json!({"case": number.to_string(), "outcome": "fail"}))
            .collect();
        run(&json!({"results": results}))
    };
    let (_, limits) = run_lines("/r", &failing(3), Some(HEAD), 2);
    assert_eq!(
        limits,
        [
            "`/r` case `0` is fail",
            "`/r` case `1` is fail",
            "and 1 more cases; see `/r`"
        ]
    );
    let (_, limits) = run_lines("/r", &failing(2), Some(HEAD), 2);
    assert_eq!(limits, ["`/r` case `0` is fail", "`/r` case `1` is fail"]);
}

#[test]
fn paths_and_the_suite_are_inline_code_and_empty_text_is_named() {
    let (line, _) = run_lines(
        "/runs/a`b",
        &run(&json!({"suite": "/suites/Fixes #1"})),
        Some(HEAD),
        20,
    );
    assert!(
        line.starts_with("`/runs/a'b`: `/suites/Fixes #1`, full run, "),
        "{line}"
    );
    let text = |value: &str| quoted(Some(&Value::from(value)));
    assert_eq!(
        (text(""), text(" \n"), text("x")),
        ("(empty)".to_owned(), "(empty)".to_owned(), "`x`".to_owned())
    );
}

#[test]
fn quoting_leaves_safe_words_bare_and_quotes_the_rest_for_a_posix_shell() {
    assert_eq!(quote("/a/b-c_d.md"), "/a/b-c_d.md");
    assert_eq!(quote(""), "''");
    assert_eq!(quote("a b"), "'a b'");
    assert_eq!(quote("it's $(rm x)"), "'it'\"'\"'s $(rm x)'");
}

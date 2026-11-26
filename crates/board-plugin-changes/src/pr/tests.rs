//! The pure parts of the pull request body: sections, a run's lines and the
//! quoting. The capability runs through the binary in board-cli's changes
//! tests.

use serde_json::{Map, Value, json};

use super::{body, quote, quoted, run_lines};
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

/// Decision 1318Z: the body names the commits after a review that left it
/// counting, and says nothing more for a review on the head. A fix commit
/// is listed apart, under the changes/fix/v1 that names it.
#[test]
fn the_review_line_names_the_commits_accepted_after_the_report() {
    let base = "0123456789abcdef0123456789abcdef01234567";
    let reviewed = "a3f9c0de1234567890abcdef1234567890abcdef";
    let later = "b3f9c0de1234567890abcdef1234567890abcdef";
    let fix = "c3f9c0de1234567890abcdef1234567890abcdef";
    let report = json!({"range": format!("{base}..{reviewed}"), "verdict": "approve",
                        "by": "a reviewer",
                        "findings": [{"severity": "low", "text": "a nit"}]});
    let fixing = json!({"report": "/reviews/r/report",
                        "commits": [{"commit": &fix[..12], "findings": [1]}]});
    let records = crate::change::Referenced {
        records: [
            (
                "/reviews/r/report".to_owned(),
                (
                    "review/report/v1".to_owned(),
                    report.as_object().unwrap().clone(),
                ),
            ),
            (
                "/changes/t/fixes/1".to_owned(),
                (
                    "changes/fix/v1".to_owned(),
                    fixing.as_object().unwrap().clone(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
        ..crate::change::Referenced::default()
    };
    let change = crate::change::from_value(json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": "land",
        "head": fix,
    }))
    .unwrap();
    let mut observed =
        crate::gates::Observed::new("/changes/t", change, crate::records::Kind::Build, records);
    let review = |observed: &crate::gates::Observed| {
        let text = body(observed, &[]);
        let line = text
            .lines()
            .find(|line| line.contains("/reviews/r/report"))
            .unwrap();
        line.to_owned()
    };
    assert!(!review(&observed).contains("accepted after it"));
    observed.after_review.insert(
        reviewed.to_owned(),
        Ok(vec![later.to_owned(), fix.to_owned()]),
    );
    assert!(
        review(&observed).ends_with(&format!(
            "; accepted after it, editing only record paths or the lifecycle doc or cleanly \
             merging main: `{later}`; accepted after it, fixing findings `/changes/t/fixes/1` \
             names: `{fix}`"
        )),
        "{}",
        review(&observed)
    );
}

/// The files the change's diff edits outside the plan's paths are listed
/// under their own heading, which is left out when there are none and says
/// why when the diff could not be listed.
#[test]
fn the_body_lists_the_files_the_change_added_to_its_plans_paths() {
    let change = crate::change::from_value(json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": "land",
    }))
    .unwrap();
    let mut observed = crate::gates::Observed::new(
        "/changes/t",
        change,
        crate::records::Kind::Build,
        crate::change::Referenced::default(),
    );
    assert!(!body(&observed, &[]).contains("## Files added to the plan's paths"));
    observed.outside = vec!["/repos/x/b.rs".to_owned(), "/repos/x/c`d.md".to_owned()];
    let text = body(&observed, &[]);
    assert!(
        text.contains(
            "## Files added to the plan's paths\n\nThe change edits these files, which no path \
             in its plan named when the plan was written:\n\n- `/repos/x/b.rs`\n- \
             `/repos/x/c'd.md`\n\n## Evidence"
        ),
        "{text}"
    );
    observed.diff_unread = Some("`git diff` failed in /r:\n bad object".to_owned());
    let text = body(&observed, &[]);
    assert!(
        text.contains(
            "## Files added to the plan's paths\n\nThe change's diff could not be listed, so the \
             files it adds to its plan's paths are not known: `git diff` failed in /r: bad \
             object. Fix that and run `board changes pr` again.\n\n## Evidence"
        ),
        "{text}"
    );
}

/// The body lists at most 50 added files and counts the rest, so a change
/// that adds a generated directory keeps its body inside GitHub's limit.
#[test]
fn the_body_lists_fifty_added_files_and_counts_the_rest() {
    let change = crate::change::from_value(json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": "land",
    }))
    .unwrap();
    let mut observed = crate::gates::Observed::new(
        "/changes/t",
        change,
        crate::records::Kind::Build,
        crate::change::Referenced::default(),
    );
    observed.outside = (0..52)
        .map(|number| format!("/repos/x/gen/{number}.rs"))
        .collect();
    let text = body(&observed, &[]);
    assert!(
        text.contains("- `/repos/x/gen/49.rs`\n- and 2 more\n"),
        "{text}"
    );
    assert!(!text.contains("/repos/x/gen/50.rs"), "{text}");
}

/// Each amendment since the last full plan is one line under its own
/// heading, before the evidence, and the heading is left out when there is
/// none.
#[test]
fn the_body_lists_what_each_amendment_added_to_the_plan() {
    let change = crate::change::from_value(json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": "land",
    }))
    .unwrap();
    let mut observed = crate::gates::Observed::new(
        "/changes/t",
        change,
        crate::records::Kind::Build,
        crate::change::Referenced::default(),
    );
    assert!(!body(&observed, &[]).contains("## Plan amendments"));
    observed.amended = vec![
        crate::amend::Amended {
            path: "/changes/t/amendments/1".to_owned(),
            from: "/changes/t/plans/1".to_owned(),
            paths: vec!["/repos/x/a.rs".to_owned(), "/repos/x/b/**".to_owned()],
            owed: vec![],
            sections: vec![],
        },
        crate::amend::Amended {
            path: "/changes/t/amendments/2".to_owned(),
            from: "/changes/t/plans/2".to_owned(),
            paths: vec![],
            owed: vec!["/rules/x/owed".to_owned()],
            sections: vec![
                crate::amend::Section::Proves,
                crate::amend::Section::DoesNotProve,
            ],
        },
    ];
    let text = body(&observed, &[]);
    assert!(
        text.contains(
            "## Plan amendments\n\nThese were added to the plan after it was written, with \
             `board changes plan-amend`:\n\n- `/changes/t/amendments/1` added the paths \
             `/repos/x/a.rs`, `/repos/x/b/**`\n- `/changes/t/amendments/2` added the owed \
             entries `/rules/x/owed`; the sections What the tests prove, What the tests do not \
             prove\n\n## Evidence"
        ),
        "{text}"
    );
}

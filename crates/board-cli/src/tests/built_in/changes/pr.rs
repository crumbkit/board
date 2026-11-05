//! `test_pr.py`: `changes.pr` over a real board, which reads the body from
//! the change, its plan and its records and gives the gh command.

use super::*;

const CHANGE: &str = "/changes/t";
const PLAN_PATH: &str = "/changes/t/plan.md";

/// A full, clean run of the head commit in which both cases pass.
fn run_record() -> Value {
    json!({
        "suite": "/suites/checks",
        "scope": "full",
        "dirty": false,
        "versions": {"x": HEAD},
        "results": [{"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "pass"}],
    })
}

/// [`run_record`] with `fields`.
fn run_with(fields: &Value) -> Value {
    serde_json::from_str(&record(run_record(), fields)).unwrap()
}

/// A change in `phase` with a filled plan, a passing run of the head
/// commit and an approving review, with `fields`.
fn landing(board: &Changes, phase: &str, fields: &Value) {
    board.write("changes/t/plan.md", PLAN);
    board.write(
        "suites/checks/runs/1.json",
        &typed("runs/run/v1", &run_record()),
    );
    board.write("reviews/1.json", &report(HEAD, "approve", BASE, &json!({})));
    let cited = json!({
        "plan": PLAN_PATH,
        "head": HEAD,
        "records": ["/suites/checks/runs/1", "/reviews/1"],
    });
    board.write(
        "changes/t.json",
        &change(
            phase,
            &serde_json::from_str(&record(cited, fields)).unwrap(),
        ),
    );
}

fn landed(board: &Changes) {
    landing(board, "land", &json!({}));
}

/// The pr.md the plugin wrote for the change.
fn pr_text(board: &Changes) -> String {
    board.text("changes/t/pr.md")
}

fn pr(board: &Changes) -> Value {
    board.call("pr", &json!({"path": CHANGE}))
}

/// The words of `command` before the body file, and that file, which must
/// be the change's pr.md.
fn split_command(board: &Changes, command: &Value) -> Vec<String> {
    let mut words = shell_words(command.as_str().unwrap());
    let file = PathBuf::from(words.pop().unwrap());
    assert_eq!(
        file.canonicalize().unwrap(),
        board.root().join("changes/t/pr.md").canonicalize().unwrap()
    );
    words
}

#[test]
fn pr_writes_the_body_beside_the_plan_and_returns_the_gh_command_that_would_use_it() {
    let board = Changes::new();
    landing(
        &board,
        "land",
        &json!({"issue": "https://github.com/o/r/issues/2"}),
    );
    let output = pr(&board);
    assert_eq!(
        split_command(&board, &output["command"]),
        [
            "gh",
            "pr",
            "create",
            "--title",
            "Let a suite declare a timeout",
            "--body-file"
        ]
    );
    assert_eq!(
        output,
        json!({
            "path": "/changes/t/pr.md",
            "command": output["command"],
            "pull_request": null,
            "unreadable": [],
        })
    );
    assert_eq!(
        pr_text(&board),
        format!(
            "A suite stops.\n\n\
             ## Why\n\nSuites hang.\n\n\
             ## Out of scope\n\nRetries.\n\n\
             ## What the tests prove\n\n\
             - `/suites/checks/runs/1`: `/suites/checks`, full run, results 2 pass\n\n\
             ## What the tests do not prove\n\n\
             _The plan's What the tests do not prove section is empty._\n\n\
             ## Review\n\n\
             - `/reviews/1`: approve by `a reviewer` over {BASE}..{HEAD}, findings none\n\n\
             ## Evidence\n\n\
             - `/changes/t`: the change\n\
             - `/changes/t/plan.md`: the plan\n\
             - `/suites/checks/runs/1`: runs/run/v1\n\
             - `/reviews/1`: review/report/v1\n\n\
             Closes https://github.com/o/r/issues/2\n"
        )
    );
}

#[test]
fn the_plans_test_sections_lead_and_each_limit_of_a_run_is_stated() {
    let board = Changes::new();
    let run = run_with(&json!({
        "scope": "targeted",
        "dirty": true,
        "versions": {"x": BASE},
        "results": [
            {"case": "a", "outcome": "pass"},
            {"case": "b", "outcome": "skipped", "note": "needs macOS"},
        ],
    }));
    landed(&board);
    board.write("suites/checks/runs/1.json", &typed("runs/run/v1", &run));
    let sections = "\n## What the tests prove\n\nTimeouts stop a suite.\n\n## What the tests do not prove\n\nSignals.\n";
    board.write("changes/t/plan.md", &format!("{PLAN}{sections}"));
    pr(&board);
    let text = pr_text(&board);
    assert!(
        text.contains(
            "## What the tests prove\n\nTimeouts stop a suite.\n\n- `/suites/checks/runs/1`: "
        ),
        "{text}"
    );
    let limits = format!(
        "## What the tests do not prove\n\nSignals.\n\n\
         - `/suites/checks/runs/1` ran only targeted cases.\n\
         - `/suites/checks/runs/1` does not record a clean tree.\n\
         - `/suites/checks/runs/1` did not run on the head commit `{HEAD}`.\n\
         - `/suites/checks/runs/1` case `b` is skipped: `needs macOS`\n\n"
    );
    assert!(text.contains(&limits), "{text}");
}

#[test]
fn a_change_without_a_run_says_no_run_is_attached_and_with_one_lists_it() {
    let board = Changes::new();
    landing(&board, "land", &json!({"records": ["/reviews/1"]}));
    pr(&board);
    let text = pr_text(&board);
    assert!(
        text.contains("- No test run is attached to this change."),
        "{text}"
    );
    landed(&board);
    pr(&board);
    let text = pr_text(&board);
    assert!(!text.contains("No test run is attached"), "{text}");
}

#[test]
fn references_that_reach_nothing_or_are_degraded_are_unreadable_and_a_plain_file_is_cited() {
    let board = Changes::new();
    landing(
        &board,
        "land",
        &json!({"records": ["/suites/checks/runs/1", "/notes/gone", "/reviews/1", "/notes/a.md"]}),
    );
    board.write("reviews/1.json", "{not json");
    board.write("notes/a.md", "# A note\n");
    let output = pr(&board);
    assert_eq!(output["unreadable"], json!(["/notes/gone", "/reviews/1"]));
    let text = pr_text(&board);
    assert!(text.contains("- `/notes/a.md`: file\n"), "{text}");
    assert!(
        text.contains("- `/notes/gone`: unreadable, board::path::not_found\n"),
        "{text}"
    );
    assert!(
        text.contains("- `/reviews/1`: unreadable, board::json::parse\n"),
        "{text}"
    );
    assert!(!text.contains("## Review"), "{text}");
}

#[test]
fn a_missing_plan_leaves_placeholders_that_name_the_empty_sections() {
    let board = Changes::new();
    landing(&board, "land", &json!({"plan": "/changes/t/none.md"}));
    pr(&board);
    let text = pr_text(&board);
    assert!(
        text.starts_with(
            "_The plan's Outcome section is empty._\n\n## Why\n\n_The plan's Problem section is empty._"
        ),
        "{text}"
    );
    assert!(
        text.contains("- `/changes/t/none.md`: the plan, missing\n"),
        "{text}"
    );
}

#[test]
fn pr_replaces_the_pr_md_an_earlier_call_wrote() {
    let board = Changes::new();
    landed(&board);
    pr(&board);
    board.write(
        "changes/t/plan.md",
        &PLAN.replace("A suite stops.", "A suite stops after its timeout."),
    );
    pr(&board);
    let text = pr_text(&board);
    assert!(
        text.starts_with("A suite stops after its timeout.\n"),
        "{text}"
    );
}

#[test]
fn a_path_that_holds_another_record_is_refused_and_the_same_path_holding_a_change_is_answered() {
    let board = Changes::new();
    board.write(
        "changes/t.json",
        &typed("plans/decision/v1", &json!({"refs": []})),
    );
    let refused = board.refusal("pr", &json!({"path": CHANGE}));
    assert_eq!(
        (&refused["code"], &refused["fields"]["path"]),
        (&json!("changes::change::not_found"), &json!(CHANGE))
    );
    assert!(!board.exists("changes/t/pr.md"));
    landed(&board);
    assert_eq!(pr(&board)["path"], "/changes/t/pr.md");
}

#[test]
fn a_change_board_marks_degraded_is_refused_and_a_valid_one_is_answered() {
    let board = Changes::new();
    landing(&board, "later", &json!({}));
    let refused = board.refusal("pr", &json!({"path": CHANGE}));
    assert_eq!(
        (&refused["code"], &refused["fields"]["code"]),
        (
            &json!("changes::change::degraded"),
            &json!("board::record::invalid")
        )
    );
    assert!(!board.exists("changes/t/pr.md"));
    landed(&board);
    assert_eq!(pr(&board)["path"], "/changes/t/pr.md");
}

#[test]
fn the_path_is_taken_as_a_flag_relative_to_the_board() {
    let board = Changes::new();
    landed(&board);
    let report = board.json(&["call", "changes.pr", "--path", "changes/t"]);
    assert_eq!(report["data"]["output"]["path"], "/changes/t/pr.md");
}

#[test]
fn record_text_cannot_add_headings_list_items_mentions_or_closing_keywords_to_the_body() {
    let board = Changes::new();
    let run = run_with(&json!({
        "summary": "Fixes #1\n## Owned by @someone",
        "results": [{"case": "b\n- c", "outcome": "fail", "note": "see `x`\n# loud"}],
    }));
    landing(
        &board,
        "land",
        &json!({"issue": "https://github.com/o/r/issues/2 Fixes #3", "title": "Two\nlines"}),
    );
    board.write("suites/checks/runs/1.json", &typed("runs/run/v1", &run));
    board.write(
        "reviews/1.json",
        &report(HEAD, "approve", BASE, &json!({"by": "@team\n# reviewer"})),
    );
    let output = pr(&board);
    assert_eq!(
        shell_words(output["command"].as_str().unwrap())[4],
        "Two lines"
    );
    let text = pr_text(&board);
    assert!(
        text.contains("summary `Fixes #1 ## Owned by @someone`\n"),
        "{text}"
    );
    assert!(
        text.contains("case `b - c` is fail: `see 'x' # loud`\n"),
        "{text}"
    );
    assert!(
        text.contains("approve by `@team # reviewer` over "),
        "{text}"
    );
    assert!(!text.contains("Closes"), "{text}");
    let injected: Vec<&str> = text
        .lines()
        .filter(|line| {
            ["## Owned", "# loud", "# reviewer", "- c"]
                .iter()
                .any(|start| line.starts_with(start))
        })
        .collect();
    assert!(injected.is_empty(), "{injected:?}");
}

#[test]
fn a_well_formed_issue_is_closed_by_the_body() {
    let board = Changes::new();
    landing(
        &board,
        "land",
        &json!({"issue": "https://github.com/o/r/issues/2"}),
    );
    pr(&board);
    let text = pr_text(&board);
    assert!(
        text.ends_with("\n\nCloses https://github.com/o/r/issues/2\n"),
        "{text}"
    );
}

#[test]
fn a_change_that_records_a_pull_request_gets_its_url_and_no_command() {
    let board = Changes::new();
    let url = "https://github.com/o/r/pull/1";
    landing(&board, "land", &json!({"pull_request": url}));
    let output = pr(&board);
    assert_eq!(
        (&output["command"], &output["pull_request"]),
        (&Value::Null, &json!(url))
    );
    assert!(!board.exists("changes/t/pr.md"));
    landed(&board);
    let output = pr(&board);
    let command = output["command"].as_str().unwrap();
    assert!(command.starts_with("gh pr create "), "{command}");
    assert_eq!(output["pull_request"], Value::Null);
    let text = pr_text(&board);
    assert!(text.starts_with("A suite stops.\n"), "{text}");
}

#[test]
fn a_reference_to_a_directory_is_cited_as_a_directory_and_one_to_nothing_is_unreadable() {
    let board = Changes::new();
    landing(
        &board,
        "land",
        &json!({"records": ["/suites/checks/runs", "/suites/none"]}),
    );
    let output = pr(&board);
    assert_eq!(output["unreadable"], json!(["/suites/none"]));
    let text = pr_text(&board);
    assert!(
        text.contains("- `/suites/checks/runs`: directory\n"),
        "{text}"
    );
    assert!(
        text.contains("- `/suites/none`: unreadable, board::path::not_found\n"),
        "{text}"
    );
}

#[test]
fn a_record_degraded_only_by_a_dangling_reference_of_its_own_is_still_read() {
    let board = Changes::new();
    landed(&board);
    board.write(
        "reviews/1.json",
        &report(HEAD, "approve", BASE, &json!({"brief": "/briefs/none"})),
    );
    assert_eq!(
        board.code("/reviews/1").as_deref(),
        Some("board::ref::dangling")
    );
    let output = pr(&board);
    assert_eq!(output["unreadable"], json!([]));
    let text = pr_text(&board);
    assert!(
        text.contains("- `/reviews/1`: approve by `a reviewer` over "),
        "{text}"
    );
    board.write("reviews/1.json", &report(HEAD, "unknown", BASE, &json!({})));
    assert_eq!(pr(&board)["unreadable"], json!(["/reviews/1"]));
}

#[test]
fn the_command_round_trips_a_title_with_shell_syntax_through_a_board_root_with_a_space() {
    let board = Changes::new();
    let title = r#"it's $(rm x) "q""#;
    landing(&board, "land", &json!({"title": title}));
    assert_eq!(
        split_command(&board, &pr(&board)["command"]),
        ["gh", "pr", "create", "--title", title, "--body-file"]
    );
}

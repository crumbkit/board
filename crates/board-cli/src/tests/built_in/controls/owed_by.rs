//! An obligation's `owed_by`: what its schema admits, what the capabilities
//! say of it, and `controls.migrate --owed_by true`, which sets it from the
//! record the obligation's own text and sources name.

use super::*;

const PULL: &str = "/gh/o/r/pulls/7";
const REPORT: &str = "/reviews/1/report";
const DECISION: &str = "/projects/a/decisions/d";
const WRONG_TYPE: Option<&str> = Some("board::ref::wrong_type");

/// A board with the plugins that own what `owed_by` may name, holding one
/// record of each type at [`PULL`], [`REPORT`] and [`DECISION`], and a task
/// file.
fn owing() -> Fixture {
    let board = Fixture::enabling(&["controls", "plans", "review", "github"]);
    let pull = json!({
        "$type": "github/pull/v1", "owner": "o", "repo": "r", "number": 7,
        "url": "https://github.com/o/r/pull/7",
    });
    let decision = json!({
        "$type": "plans/decision/v1", "decision": "Do it.", "why": "It is needed.",
        "by": {"session": "local_0a1b"}, "decided": "2026-10-01T09:00:00Z",
    });
    let report = json!({
        "$type": "review/report/v1", "range": "a..b", "patch_ids": ["p"], "findings": [],
        "verdict": "approve", "by": "a reviewer",
    });
    board
        .write("gh/o/r/pulls/7.json", &pull.to_string())
        .write("projects/a/decisions/d.json", &decision.to_string())
        .write("projects/a/decisions/e.json", &decision.to_string())
        .write("reviews/1/report.json", &report.to_string())
        .write("notes/task.md", "# A task\n");
    board
}

fn obligation(requirement: &str, fields: &Value) -> String {
    let mut body: Value = serde_json::from_str(&control(requirement, fields)).unwrap();
    body["kind"] = json!("obligation");
    body.to_string()
}

#[test]
fn an_owed_by_naming_a_pull_a_report_or_a_decision_passes_and_one_naming_a_file_is_the_wrong_type()
{
    let board = owing();
    for (name, target) in [("pull", PULL), ("report", REPORT), ("decision", DECISION)] {
        board.write(
            &format!("controls/x/{name}.json"),
            &obligation("Owed.", &json!({"owed_by": target})),
        );
    }
    board.write(
        "controls/x/task.json",
        &obligation("Owed.", &json!({"owed_by": "/notes/task.md"})),
    );
    assert_eq!(
        board.checked("/controls/x/"),
        codes(&[
            ("/controls/x/pull", None),
            ("/controls/x/report", None),
            ("/controls/x/decision", None),
            ("/controls/x/task", WRONG_TYPE),
        ])
    );
}

#[test]
fn check_warns_of_a_standing_control_with_owed_by_and_its_obligation_twin_does_not_warn() {
    let board = owing();
    board.write(
        "controls/x/rule.json",
        &control(
            "Keep it.",
            &json!({"owed_by": DECISION, "applies_to": ["/notes/task.md"]}),
        ),
    );
    let warned = board.call("controls.check", &[]);
    assert_eq!(
        warned["warnings"][0]["code"],
        "controls::control::owed_by_standing"
    );
    assert_eq!(warned["stated"][0].get("owed_by"), None);
    board.write(
        "controls/x/rule.json",
        &obligation(
            "Keep it.",
            &json!({"owed_by": DECISION, "applies_to": ["/notes/task.md"]}),
        ),
    );
    let quiet = board.call("controls.check", &[]);
    assert_eq!(quiet["warnings"], json!([]));
    assert_eq!(quiet["stated"][0]["owed_by"], DECISION);
}

#[test]
fn for_path_and_for_paths_return_what_an_obligation_is_owed_by() {
    let board = owing();
    board.write(
        "controls/x/owed.json",
        &obligation(
            "Owed.",
            &json!({"owed_by": DECISION, "applies_to": ["/notes/task.md"]}),
        ),
    );
    let one = board.call("controls.for-path", &["--path", "/notes/task.md"]);
    assert_eq!(one["controls"][0]["owed_by"], DECISION);
    let arguments = ["--json", "call", "controls.for-paths"];
    let output = board.run_with(&arguments, r#"{"paths": ["/notes/task.md"]}"#);
    assert_eq!(
        json(&output)["data"]["output"]["controls"][0]["owed_by"],
        DECISION
    );
}

/// A repository whose origin is acme/lib, mounted at `/repos/lib`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test gives the mounted repository an origin with git"
)]
fn mounted_library(board: &Fixture) -> Scenario {
    let library = Scenario::new();
    let commit = library.commit(&[("README.md", "lib\n")]);
    let origin = std::process::Command::new("git")
        .args(["remote", "add", "origin", "https://github.com/acme/lib.git"])
        .current_dir(library.workdir())
        .output()
        .unwrap();
    assert!(origin.status.success(), "{origin:?}");
    mount(board, "repos/lib.json", &library, commit);
    library
}

/// The controls the migration tests hold, by the case each stands for.
fn obligations_to_migrate(board: &Fixture) -> Scenario {
    let library = mounted_library(board);
    let pull = json!({
        "$type": "github/pull/v1", "owner": "acme", "repo": "lib", "number": 9,
        "url": "https://github.com/acme/lib/pull/9",
    });
    board.write("gh/acme/lib/pulls/9.json", &pull.to_string());
    let cases = [
        ("named-pull", "Owed by the merge of o/r#7.", json!({})),
        ("missing-pull", "Owed by the merge of o/r#8.", json!({})),
        (
            "mount-pull",
            "Owed by the merge of PR #9.",
            json!({"applies_to": ["/repos/lib/README.md"]}),
        ),
        ("two-pulls", "Owed by o/r#7 and o/r#8.", json!({})),
        (
            "report",
            "Owed by a review.",
            json!({"sources": [REPORT, DECISION]}),
        ),
        (
            "decision",
            "Owed by a review.",
            json!({"sources": [DECISION, "/notes/task.md"]}),
        ),
        (
            "two-decisions",
            "Owed by a review.",
            json!({"sources": [DECISION, "/projects/a/decisions/e"]}),
        ),
        ("nothing", "Owed by a delivery step.", json!({})),
        ("set", "Owed by o/r#7.", json!({"owed_by": DECISION})),
    ];
    for (name, requirement, fields) in cases {
        board.write(
            &format!("controls/x/{name}.json"),
            &obligation(requirement, &fields),
        );
    }
    board.write(
        "controls/x/standing.json",
        &control("Owed by o/r#7.", &json!({})),
    );
    board.write(
        "controls/x/unread.json",
        &json!({"$type": "controls/control/v1", "kind": "obligation", "requirement": 5, "purpose": "p"})
            .to_string(),
    );
    library
}

fn owed_by(board: &Fixture, name: &str) -> Value {
    let body: Value =
        serde_json::from_str(&board.read(&format!("controls/x/{name}.json"))).unwrap();
    body.get("owed_by").cloned().unwrap_or(Value::Null)
}

#[test]
fn migrate_lists_what_it_would_set_and_writes_nothing_without_apply() {
    let board = owing();
    let _library = obligations_to_migrate(&board);
    board.committing();
    let before = board.head();
    let dry = board.call("controls.migrate", &["--owed_by", "true"]);
    assert_eq!(dry["applied"], false);
    assert_eq!(
        dry["rewritten"],
        json!([
            {"path": "/controls/x/decision", "owed_by": DECISION},
            {"path": "/controls/x/mount-pull", "owed_by": "/gh/acme/lib/pulls/9"},
            {"path": "/controls/x/named-pull", "owed_by": PULL},
            {"path": "/controls/x/report", "owed_by": REPORT},
        ])
    );
    assert_eq!(
        dry["commands"],
        json!(["board call github.ref --ref o/r#8"])
    );
    let ambiguous = dry["ambiguous"].as_array().unwrap();
    let paths: Vec<&str> = (ambiguous.iter())
        .map(|each| each["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["/controls/x/two-decisions", "/controls/x/two-pulls"]
    );
    assert_eq!(ambiguous[0]["code"], "controls::migrate::ambiguous");
    assert_eq!(dry["unmatched"], json!(["/controls/x/nothing"]));
    assert_eq!(dry["unread"][0]["path"], "/controls/x/unread");
    assert_eq!(dry["unread"][0]["code"], "controls::migrate::unread");
    assert_eq!(board.head(), before);
}

#[test]
fn migrate_with_apply_writes_one_bulk_commit_and_a_second_apply_writes_nothing() {
    let board = owing();
    let _library = obligations_to_migrate(&board);
    board.committing();
    let before = board.head();
    let applied = board.call(
        "controls.migrate",
        &["--owed_by", "true", "--apply", "true"],
    );
    assert_eq!(
        (&applied["applied"], &applied["total"]),
        (&json!(true), &json!(4))
    );
    assert_eq!(board.board.head().unwrap().parents, [before]);
    assert!(
        board
            .message()
            .ends_with("\n\nBoard-Bulk: migrate controls"),
        "{}",
        board.message()
    );
    assert_eq!(owed_by(&board, "named-pull"), PULL);
    assert_eq!(owed_by(&board, "mount-pull"), "/gh/acme/lib/pulls/9");
    assert_eq!(owed_by(&board, "report"), REPORT);
    assert_eq!(owed_by(&board, "decision"), DECISION);
    for name in [
        "missing-pull",
        "two-pulls",
        "two-decisions",
        "nothing",
        "standing",
    ] {
        assert_eq!(owed_by(&board, name), Value::Null, "{name}");
    }
    assert_eq!(owed_by(&board, "set"), DECISION);
    let head = board.head();
    let again = board.call(
        "controls.migrate",
        &["--owed_by", "true", "--apply", "true"],
    );
    assert_eq!(again["total"], 0);
    assert_eq!(board.head(), head);
}

#[test]
fn migrate_without_owed_by_writes_nothing_and_says_so() {
    let board = owing();
    let _library = obligations_to_migrate(&board);
    board.committing();
    let before = board.head();
    let output = board.call("controls.migrate", &["--apply", "true"]);
    assert_eq!(output["total"], 0);
    assert_eq!(board.head(), before);
}

#[test]
fn a_number_the_board_holds_as_an_issue_is_no_match_so_the_sources_decide_and_a_pull_still_matches()
{
    let board = owing();
    let issue = json!({
        "$type": "github/issue/v1", "owner": "o", "repo": "r", "number": 5,
        "url": "https://github.com/o/r/issues/5",
    });
    board.write("gh/o/r/issues/5.json", &issue.to_string());
    for (name, requirement, fields) in [
        (
            "issue-source",
            "Owed by o/r#5.",
            json!({"sources": [DECISION]}),
        ),
        ("issue-alone", "Owed by o/r#5.", json!({})),
        ("pull", "Owed by o/r#7.", json!({})),
    ] {
        board.write(
            &format!("controls/x/{name}.json"),
            &obligation(requirement, &fields),
        );
    }
    let dry = board.call("controls.migrate", &["--owed_by", "true"]);
    assert_eq!(
        dry["rewritten"],
        json!([
            {"path": "/controls/x/issue-source", "owed_by": DECISION},
            {"path": "/controls/x/pull", "owed_by": PULL},
        ])
    );
    assert_eq!(dry["commands"], json!([]));
    assert_eq!(dry["unmatched"], json!(["/controls/x/issue-alone"]));
}

#[test]
fn a_bare_number_counts_only_after_a_word_that_names_a_pull_request() {
    let board = owing();
    let _library = mounted_library(&board);
    let pull = json!({
        "$type": "github/pull/v1", "owner": "acme", "repo": "lib", "number": 3,
        "url": "https://github.com/acme/lib/pull/3",
    });
    board.write("gh/acme/lib/pulls/3.json", &pull.to_string());
    for (name, requirement) in [
        ("finding", "Finding #3 stays open."),
        ("pr", "Owed by PR #3."),
        ("pull", "Owed by pull #3."),
        ("request", "Owed by pull request #3."),
        ("both", "Owed by PR #3 and #4."),
        ("listed", "Owed by pull requests #3, #4."),
    ] {
        board.write(
            &format!("controls/x/{name}.json"),
            &obligation(
                requirement,
                &json!({"applies_to": ["/repos/lib/README.md"]}),
            ),
        );
    }
    let dry = board.call("controls.migrate", &["--owed_by", "true"]);
    let at = "/gh/acme/lib/pulls/3";
    assert_eq!(
        dry["rewritten"],
        json!([
            {"path": "/controls/x/pr", "owed_by": at},
            {"path": "/controls/x/pull", "owed_by": at},
            {"path": "/controls/x/request", "owed_by": at},
        ])
    );
    assert_eq!(dry["unmatched"], json!(["/controls/x/finding"]));
    let ambiguous: Vec<&str> = (dry["ambiguous"].as_array().unwrap().iter())
        .map(|each| each["path"].as_str().unwrap())
        .collect();
    assert_eq!(ambiguous, ["/controls/x/both", "/controls/x/listed"]);
}

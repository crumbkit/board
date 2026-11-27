//! `changes.ready` through the real index: which changes wait only on the
//! owner, the kind of check each waits on, their order, and why one is
//! failing.

use super::rechecks::{TESTED, built};
use super::*;

/// The fixture board with the plans plugin enabled too, so questions
/// validate.
fn with_plans() -> Changes {
    Changes::enabling(&["changes", "rules", "plans"])
}

/// Writes the change `/changes/<name>` in land, touched `days` ago, whose
/// land gate passes when `pull` is true, with `fields`.
fn landing(board: &Changes, name: &str, days: i64, pull: bool, fields: &Value) {
    let mut given = json!({"touched": days_ago(days), "route": "owner", "records": []});
    if pull {
        given["pull_request"] = json!(format!("https://github.com/o/r/pull/{}", name.len()));
    }
    for (key, value) in fields.as_object().unwrap() {
        given[key] = value.clone();
    }
    board.change_at(&format!("changes/{name}.json"), "land", &given);
    board.write(
        &format!("changes/{name}/plans/1.md"),
        &format!("{PLAN}{TESTED}"),
    );
}

/// Writes an open question at `/projects/p/questions/<name>` that blocks
/// `blocks`, and returns its path.
fn asking(board: &Changes, name: &str, blocks: &str) -> String {
    let question = typed(
        "plans/question/v1",
        &json!({
            "question": format!("Land {blocks} now?"),
            "by": {"session": "local_0a1b"},
            "asked": "2026-11-20T09:00:00Z",
            "status": "open",
            "blocks": blocks,
        }),
    );
    board.write(&format!("projects/p/questions/{name}.json"), &question);
    format!("/projects/p/questions/{name}")
}

/// What `changes.ready` lists, by path.
fn ready_paths(board: &Changes, given: &Value) -> Vec<String> {
    let output = board.call_stdin("ready", given);
    (output["changes"].as_array().unwrap().iter())
        .map(|each| each["path"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_land_change_an_open_question_blocks_is_ready_until_an_answer_names_the_question() {
    let board = with_plans();
    landing(&board, "t", 2, true, &json!({}));
    let question = asking(&board, "q", "/changes/t");
    let output = board.call_stdin("ready", &json!({}));
    assert_eq!(
        output,
        json!({
            "changes": [{
                "path": "/changes/t",
                "title": "Let a suite declare a timeout",
                "pull": "/gh/o/r/pulls/1",
                "head": null,
                "kind": "land-choice",
                "check": null,
                "waits_on": "Land /changes/t now?",
                "questions": [question],
                "days": 2,
                "failing": null,
                "done": false,
            }],
            "total": 1,
            "truncated": false,
            "unreadable": 0,
        })
    );
    board.write(
        "projects/p/answers/q.json",
        &typed(
            "plans/answer/v1",
            &json!({"question": question, "choice": "a", "words": "Yes.",
                    "answered": "2026-11-21T09:00:00Z", "by": {"person": PERSON}}),
        ),
    );
    assert_eq!(ready_paths(&board, &json!({})), Vec::<String>::new());
}

/// A missing check of the agent's means the change is not built yet, so a
/// question that blocks it does not make it ready; its twin with the check
/// passing is ready.
#[test]
fn a_land_change_missing_a_check_of_the_agents_is_not_ready_and_its_twin_that_passes_it_is() {
    let board = with_plans();
    landing(&board, "t", 0, false, &json!({}));
    landing(&board, "u", 0, true, &json!({}));
    asking(&board, "q", "/changes/t");
    asking(&board, "r", "/changes/u");
    assert_eq!(ready_paths(&board, &json!({})), ["/changes/u"]);
}

/// With nothing for the owner to do, a land change whose gate passes
/// waits on its merge, not on the owner.
#[test]
fn a_land_change_whose_gate_passes_with_no_question_is_not_ready() {
    let board = with_plans();
    landing(&board, "t", 0, true, &json!({}));
    assert_eq!(ready_paths(&board, &json!({})), Vec::<String>::new());
}

/// The owner's rechecks do not block the land gate, so a change missing
/// only them is ready as a recheck, and it stays a recheck row once done,
/// as `changes.owed` lists it.
#[test]
fn a_change_missing_only_its_rechecks_is_a_recheck_row_in_land_and_once_done() {
    let board = built(&[Some("first-point")]);
    let row = |board: &Changes| {
        let output = board.call_stdin("ready", &json!({}));
        assert_eq!(output["total"], 1, "{output}");
        let row = output["changes"][0].clone();
        (
            row["path"].clone(),
            row["kind"].clone(),
            row["check"].clone(),
            row["pull"].clone(),
        )
    };
    let expected = (
        json!("/changes/t"),
        json!("recheck"),
        json!("rechecks"),
        json!("/gh/o/r/pulls/7"),
    );
    assert_eq!(row(&board), expected);
    board.call("advance", &json!({"path": "/changes/t"}));
    assert_eq!(row(&board), expected);
}

/// The owner's rechecks apply only when the plan answers that the change
/// stores data, so a change whose plan answers `none` is no recheck row in
/// land or once done.
#[test]
fn a_change_whose_plan_stores_no_data_owes_no_recheck_in_land_or_once_done() {
    let board = built(&[Some("first-point")]);
    let mut plan = board.read("changes/t/plans/1.json");
    plan["answers"]["data"] = json!("none");
    board.write("changes/t/plans/1.json", &plan.to_string());
    assert_eq!(ready_paths(&board, &json!({})), Vec::<String>::new());
    board.call("advance", &json!({"path": "/changes/t"}));
    assert_eq!(ready_paths(&board, &json!({})), Vec::<String>::new());
}

/// Rows come oldest first and failing ones last. A head that conflicts
/// with main is failing; a head only behind main is not.
#[test]
fn a_ready_change_whose_head_conflicts_with_main_is_failing_and_listed_last() {
    let board = with_plans();
    let (_, head) = board.edited(&[("notes.md", "work\n")]);
    landing(&board, "t", 1, true, &json!({}));
    landing(&board, "u", 3, true, &json!({"head": head}));
    asking(&board, "q", "/changes/t");
    asking(&board, "r", "/changes/u");
    // main moves on without touching the change's file: u is only behind.
    board.commit(&[("other.md", "main\n")], Some("main"));
    let output = board.call_stdin("ready", &json!({}));
    let rows: Vec<(&Value, &Value)> = (output["changes"].as_array().unwrap().iter())
        .map(|each| (&each["path"], &each["failing"]))
        .collect();
    assert_eq!(
        rows,
        [
            (&json!("/changes/u"), &Value::Null),
            (&json!("/changes/t"), &Value::Null)
        ]
    );
    // main now writes the same file differently: u conflicts.
    board.commit(&[("notes.md", "main\n")], Some("main"));
    let output = board.call_stdin("ready", &json!({}));
    let rows: Vec<(&Value, &Value)> = (output["changes"].as_array().unwrap().iter())
        .map(|each| (&each["path"], &each["failing"]))
        .collect();
    assert_eq!(
        rows,
        [
            (&json!("/changes/t"), &Value::Null),
            (&json!("/changes/u"), &json!("conflict"))
        ]
    );
    // Without the failing reads, u is not marked and comes first again.
    assert_eq!(
        ready_paths(&board, &json!({"failing": false})),
        ["/changes/u", "/changes/t"]
    );
}

/// The newest CI run at the head failing makes a change failing, and so
/// does a newest review that covers another commit; the twin of each
/// with a passing run and a review of the head is not failing.
#[test]
fn a_ready_change_whose_newest_run_failed_or_whose_review_is_of_another_commit_is_failing() {
    let board = with_plans();
    let (base, head) = board.edited(&[("notes.md", "work\n")]);
    let run = |name: &str, outcome: &str, finished: &str| {
        board.write(
            &format!("suites/s/runs/{name}.json"),
            &typed(
                "runs/run/v1",
                &json!({"versions": {"x": head}, "finished": finished,
                        "results": [{"case": "ci", "outcome": outcome}]}),
            ),
        );
        format!("/suites/s/runs/{name}")
    };
    let failed = run("1", "fail", "2026-11-24T10:00:00Z");
    let passed = run("2", "pass", "2026-11-23T10:00:00Z");
    board.write(
        "reviews/head/report.json",
        &report(&head, "approve", &base, &json!({})),
    );
    board.write(
        "reviews/base/report.json",
        &report(&base, "approve", &base, &json!({})),
    );
    let failing = |records: &[&str]| {
        landing(
            &board,
            "t",
            0,
            true,
            &json!({"head": head, "records": records}),
        );
        let output = board.call_stdin("ready", &json!({}));
        output["changes"][0]["failing"].clone()
    };
    asking(&board, "q", "/changes/t");
    assert_eq!(failing(&[&passed, &failed]), json!("ci_failed"));
    let newer = run("3", "pass", "2026-11-25T10:00:00Z");
    assert_eq!(failing(&[&passed, &failed, &newer]), Value::Null);
    assert_eq!(failing(&[&newer, "/reviews/base/report"]), json!("stale"));
    // A report of the head covers it, in whichever order the reports sort.
    assert_eq!(
        failing(&[&newer, "/reviews/head/report", "/reviews/base/report"]),
        Value::Null
    );
    assert_eq!(
        failing(&[&newer, "/reviews/base/report", "/reviews/head/report"]),
        Value::Null
    );
}

/// When git cannot say whether the head merges, the change is not marked
/// failing, even with a review of another commit, and a warning names it.
#[test]
fn a_ready_change_whose_merge_check_git_cannot_answer_is_not_failing_and_warns() {
    let board = with_plans();
    let (base, _) = board.edited(&[("notes.md", "work\n")]);
    board.write(
        "reviews/base/report.json",
        &report(&base, "approve", &base, &json!({})),
    );
    let absent = "0123456789abcdef0123456789abcdef01234567";
    landing(
        &board,
        "t",
        0,
        true,
        &json!({"head": absent, "records": ["/reviews/base/report"]}),
    );
    asking(&board, "q", "/changes/t");
    let output = board.call_stdin("ready", &json!({}));
    assert_eq!(output["changes"][0]["failing"], Value::Null, "{output}");
    assert_eq!(
        output["warnings"][0]["code"],
        "changes::ready::merge_unread"
    );
}

/// A board holding the change `/changes/a` in land on the flow `own`,
/// whose one owner check does not block, is missing while the change
/// records no head, and applies `when` the plan's Data section matches
/// `pattern` and its answers say it stores data. The plan answers that it
/// stores none.
fn sectioned(pattern: &str) -> Changes {
    let board = Changes::new();
    let check = json!({
        "id": "signed", "kind": "field", "field": "head", "actor": "owner", "blocks": false,
        "when": {"section": {"Data": pattern}, "answers": {"data": "stored"}},
    });
    board.write(
        "flows/own.json",
        &json!({
            "$type": "changes/flow/v1",
            "name": "own",
            "summary": "A flow for a test.",
            "stop": {"through": "land"},
            "phases": [{"name": "land", "agent": "lead", "instruction": "Land it.",
                        "checks": [check]}],
        })
        .to_string(),
    );
    board.write(
        "changes/a/plans/1.md",
        "# A\n\n## Data\n\nOne record is stored.\n",
    );
    board.write(
        "changes/a/plans/1.json",
        &typed(
            "changes/plan/v1",
            &json!({"text": "/changes/a/plans/1.md", "uses": [],
                    "answers": {"ux": "none", "data": "none", "lifecycle": "none"}}),
        ),
    );
    board.write(
        "changes/a.json",
        &typed(
            "changes/change/v2",
            &json!({"title": "A landing change", "flow": "/flows/own", "phase": "land",
                    "route": "owner", "touched": today(), "plan": "/changes/a/plans/1"}),
        ),
    );
    assert_eq!(board.code("/changes/a"), None);
    board
}

/// The gate leaves a `when` with a section pattern to the pattern whatever
/// the answers, and a pattern that is not a regular expression makes the
/// owner's check missing, so the change waits on the owner and is listed.
/// Its twin, whose pattern matches, fails on the answers and is not.
#[test]
fn an_owner_check_whose_section_pattern_cannot_be_read_is_ready_as_the_gate_says_and_its_twin_is_not()
 {
    let board = sectioned("(");
    let next = board.call("next", &json!({"path": "/changes/a"}));
    let signed = (next["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "signed")
        .unwrap();
    assert_eq!(signed["status"], "missing", "{next}");
    assert_eq!(ready_paths(&board, &json!({})), ["/changes/a"]);
    let board = sectioned("stored");
    assert_eq!(ready_paths(&board, &json!({})), Vec::<String>::new());
}

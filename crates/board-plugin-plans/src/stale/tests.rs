//! `stale` lists the changes, evals and tasks nobody has touched, and files
//! one drop-or-keep question for each; its twins that someone owns, that
//! ended or that saw recent activity are left out.

use std::time::{Duration, SystemTime};

use board_model::utc_seconds;
use board_ops::{Board, Plugins};
use board_plugin::Target;
use serde_json::{Value, json};

use crate::open::tests::{answer_naming, body, mounting_board, plans_board};

const OLD: &str = "2026-01-01";
const SESSION: &str = "local_aa";

/// The UTC day `days` before today.
fn ago(days: u64) -> String {
    let then = SystemTime::now() - Duration::from_secs(days * 86_400);
    utc_seconds(then)[..10].to_owned()
}

/// What `plans.stale` returns for `input` on `board`.
fn stale(board: &Board, input: &Value) -> Value {
    let target: Target = "plans.stale".parse().unwrap();
    let input = input.to_string();
    board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap()
        .output
}

/// The code of the refusal of `plans.stale` for `input` on `board`.
fn refused(board: &Board, input: &Value) -> String {
    let target: Target = "plans.stale".parse().unwrap();
    let input = input.to_string();
    let error = board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap_err();
    error.refusal().code.as_str().to_owned()
}

/// The paths of the items listed, sorted.
fn listed(output: &Value) -> Vec<String> {
    let mut paths: Vec<String> = (output["items"].as_array().unwrap().iter())
        .map(|item| item["path"].as_str().unwrap().to_owned())
        .collect();
    paths.sort();
    paths
}

/// A file's text from `fields`, which a record holds.
fn record(fields: &Value) -> Value {
    Value::String(fields.to_string())
}

fn change(phase: &str, route: &str, touched: &str) -> Value {
    record(&json!({
        "$type": "changes/change/v2", "title": "Work", "phase": phase, "route": route,
        "touched": touched,
    }))
}

fn eval(status: &str, runs: &[&str]) -> Value {
    let mut fields = json!({
        "$type": "evals/eval/v1", "question": "Does it work?", "subjects": ["a/b"],
        "status": status, "runs": runs,
    });
    if status == "concluded" {
        fields["verdict"] = json!("supported");
    }
    record(&fields)
}

fn run(started: &str) -> Value {
    record(&json!({"$type": "runs/run/v1", "suite": "/suites/s", "started": started}))
}

fn task(fields: &Value) -> Value {
    let mut all = json!({"$type": "tasks/task/v2", "title": "Do it", "status": "open"});
    for (name, value) in fields.as_object().unwrap() {
        all[name] = value.clone();
    }
    record(&all)
}

#[test]
fn an_unowned_change_in_plan_without_recent_activity_is_listed_and_its_twins_are_not() {
    let fresh = format!("{}T09:00:00Z", ago(2));
    let (_scenario, board) = plans_board(&[
        ("changes/stale", change("plan", "unowned", OLD)),
        ("changes/owned", change("plan", "owner", OLD)),
        ("changes/building", change("build", "unowned", OLD)),
        ("changes/done", change("done", "unowned", OLD)),
        ("changes/fresh", change("plan", "unowned", &fresh[..10])),
    ]);
    let output = stale(&board, &json!({}));
    assert_eq!(listed(&output), vec!["/changes/stale"]);
    assert_eq!(output["counts"]["changes"], json!(1));
    let item = &output["items"][0];
    assert_eq!(item["kind"], json!("change"));
    assert_eq!(item["last_activity"], json!(OLD));
    assert_eq!(item["recommendation"], json!("drop"));
    assert!(item["reason"].as_str().unwrap().contains(OLD));
}

#[test]
fn an_eval_planned_or_running_without_a_verdict_is_listed_unless_a_run_is_recent() {
    let recent = format!("{}T09:00:00Z", ago(1));
    let (_scenario, board) = plans_board(&[
        ("evals/planned", eval("planned", &[])),
        ("evals/running", eval("running", &["/suites/s/runs/old"])),
        (
            "evals/active",
            eval("running", &["/suites/s/runs/old", "/suites/s/runs/new"]),
        ),
        ("evals/concluded", eval("concluded", &[])),
        ("evals/abandoned", eval("abandoned", &[])),
        ("suites/s/runs/old", run("2026-01-02T09:00:00Z")),
        ("suites/s/runs/new", run(&recent)),
    ]);
    let output = stale(&board, &json!({}));
    assert_eq!(listed(&output), vec!["/evals/planned", "/evals/running"]);
    let running = (output["items"].as_array().unwrap().iter())
        .find(|item| item["path"] == json!("/evals/running"))
        .unwrap();
    assert_eq!(running["last_activity"], json!("2026-01-02"));
}

#[test]
fn an_open_task_nobody_claimed_for_a_month_is_listed_and_its_twins_are_not() {
    let (_scenario, board) = plans_board(&[
        ("tasks/old", task(&json!({"reviewed": OLD}))),
        (
            "tasks/claimed",
            task(&json!({"reviewed": OLD, "owner": {"person": "ana"}})),
        ),
        (
            "tasks/project",
            task(&json!({"reviewed": OLD, "kind": "project"})),
        ),
        (
            "tasks/deferred",
            task(&json!({"reviewed": OLD, "defer": "2027-01-01"})),
        ),
        (
            "tasks/done",
            task(&json!({"reviewed": OLD, "status": "done"})),
        ),
        ("tasks/week", task(&json!({"reviewed": ago(10)}))),
        ("tasks/new", task(&json!({"reviewed": ago(3)}))),
    ]);
    let output = stale(&board, &json!({}));
    assert_eq!(listed(&output), vec!["/tasks/old"]);
    assert_eq!(
        output["counts"],
        json!({"changes": 0, "evals": 0, "tasks": 1})
    );
    let output = stale(&board, &json!({"task_days": 7}));
    assert_eq!(listed(&output), vec!["/tasks/old", "/tasks/week"]);
}

#[test]
fn without_apply_nothing_is_filed_and_with_it_each_item_becomes_one_drop_or_keep_question() {
    let (_scenario, board) = plans_board(&[
        ("changes/stale", change("plan", "unowned", OLD)),
        ("tasks/old", task(&json!({"reviewed": OLD}))),
    ]);
    stale(&board, &json!({"session": SESSION}));
    assert!(open(&board).is_empty());
    let output = stale(&board, &json!({"session": SESSION, "apply": true}));
    assert_eq!(output["outcomes"], json!({"filed": 2}));
    let questions = open(&board);
    assert_eq!(questions.len(), 2);
    let first = (questions.iter())
        .find(|each| each["question"] == json!("Drop change /changes/stale?"))
        .unwrap();
    assert_eq!(first["recommendation"], json!("drop"));
    let filed = output["items"][0]["question"].as_str().unwrap();
    assert!(filed.starts_with("/projects/board/questions/"), "{filed}");
    let ids: Vec<&str> = (first["options"].as_array().unwrap().iter())
        .map(|option| option["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["drop", "keep"]);
    assert!(first["why"].as_str().unwrap().contains(OLD));
}

#[test]
fn a_second_sweep_merges_into_the_open_questions_and_an_answered_item_is_not_asked_again() {
    let (_scenario, board) = plans_board(&[
        ("changes/stale", change("plan", "unowned", OLD)),
        ("changes/kept", change("plan", "unowned", OLD)),
        (
            "projects/board/questions/kept",
            body(
                "2026-10-01T09:00:00Z",
                &json!({"question": "Drop change /changes/kept?", "by": {"session": "local_zz"}}),
            ),
        ),
        (
            "projects/board/answers/kept",
            answer_naming("/projects/board/questions/kept"),
        ),
    ]);
    let first = stale(&board, &json!({"session": SESSION, "apply": true}));
    assert_eq!(first["outcomes"], json!({"filed": 1}));
    assert_eq!(first["already_answered"], json!(1));
    assert_eq!(listed(&first), vec!["/changes/stale"]);
    let second = stale(&board, &json!({"session": "local_bb", "apply": true}));
    assert_eq!(second["outcomes"], json!({"merged": 1}));
    assert_eq!(open(&board).len(), 1);
}

#[test]
fn apply_without_a_session_is_refused_and_the_twin_with_one_is_admitted() {
    let (_scenario, board) = plans_board(&[("changes/stale", change("plan", "unowned", OLD))]);
    assert_eq!(
        refused(&board, &json!({"apply": true})),
        "plans::stale::no_session"
    );
    assert!(open(&board).is_empty());
    stale(&board, &json!({"apply": true, "session": SESSION}));
    assert_eq!(open(&board).len(), 1);
}

/// The open questions `plans.open` lists.
fn open(board: &Board) -> Vec<Value> {
    let target: Target = "plans.open".parse().unwrap();
    let output = board
        .call(&target, b"{}", &Plugins::default())
        .unwrap()
        .output;
    output["questions"].as_array().unwrap().clone()
}

/// A commit within the window that touched a change's folder or an eval's
/// file counts as activity, even when the record's own days are old. The
/// twins nobody committed are listed.
#[test]
fn a_recent_commit_keeps_a_record_out_unless_it_is_a_bulk_commit() {
    let (scenario, board) = plans_board(&[
        ("changes/stale", change("plan", "unowned", OLD)),
        ("changes/committed", change("plan", "unowned", OLD)),
        ("changes/folder", change("plan", "unowned", OLD)),
        ("changes/bulk", change("plan", "unowned", OLD)),
        ("evals/stale", eval("planned", &[])),
        ("evals/committed", eval("planned", &[])),
        ("tasks/stale", task(&json!({"reviewed": OLD}))),
        ("tasks/committed", task(&json!({"reviewed": OLD}))),
    ]);
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let day_ago = now - 86_400;
    let bulk = scenario.commit_at(
        &[("changes/bulk.json", "{}")],
        &[],
        day_ago - 1,
        "rename\n\nBoard-Bulk: rename controls to rules",
    );
    scenario.commit_at(
        &[
            ("changes/bulk.json", "{}"),
            ("changes/committed.json", "{}"),
            ("changes/folder/plans/p.md", "plan"),
            ("evals/committed.json", "{}"),
            ("tasks/committed.json", "{}"),
        ],
        &[bulk],
        day_ago,
        "recent",
    );
    let output = stale(&board, &json!({}));
    assert_eq!(
        listed(&output),
        vec![
            "/changes/bulk",
            "/changes/stale",
            "/evals/stale",
            "/tasks/stale"
        ]
    );
}

/// The sweep reads the board's own records only: a stale change in a
/// mounted board is left for that board's own sweep. The twin in the board
/// itself is listed.
#[test]
fn a_mounted_boards_records_are_left_to_its_own_sweep() {
    let (_root, _library, board) = mounting_board(
        &[("changes/own", change("plan", "unowned", OLD))],
        &[("changes/theirs", change("plan", "unowned", OLD))],
    );
    assert_eq!(listed(&stale(&board, &json!({}))), vec!["/changes/own"]);
}

/// A change whose plan was written recently is active, whatever its
/// `touched` says, and `days` moves the window for changes.
#[test]
fn a_recent_plan_keeps_a_change_out_and_days_moves_the_window() {
    let plan = format!(
        "/changes/planned/plans/{}T093000123456Z",
        ago(1).replace('-', "")
    );
    let mut planned =
        serde_json::from_str::<Value>(change("plan", "unowned", OLD).as_str().unwrap()).unwrap();
    planned["plan"] = json!(plan);
    let (_scenario, board) = plans_board(&[
        ("changes/planned", record(&planned)),
        ("changes/touched", change("plan", "unowned", &ago(10))),
    ]);
    assert_eq!(listed(&stale(&board, &json!({}))), vec!["/changes/touched"]);
    assert!(listed(&stale(&board, &json!({"days": 30}))).is_empty());
}

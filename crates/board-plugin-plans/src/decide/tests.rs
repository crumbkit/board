//! `decide` writes one `plans/decision/v1` from its flags, refuses a call
//! that names no author, and warns when its project holds nothing else.

use std::rc::Rc;
use std::time::SystemTime;

use board_model::utc_seconds;
use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::open::tests::{body, host, plans_board};

/// What `plans.decide` returns for `input` on `board`.
fn decide(board: &Board, input: &Value) -> Value {
    let target: Target = "plans.decide".parse().unwrap();
    let input = input.to_string();
    board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap()
        .output
}

/// The code Board refuses `plans.decide` with for `input` on `board`.
fn refused(board: &Board, input: &Value) -> board_model::Refusal {
    let target: Target = "plans.decide".parse().unwrap();
    let input = input.to_string();
    board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap_err()
        .refusal()
}

/// The record stored at the logical `path` of `scenario`'s board.
fn stored(scenario: &Scenario, path: &str) -> Value {
    let file = scenario.workdir().join(format!("{}.json", &path[1..]));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

/// A board whose project `a` already holds a question, so a decision there
/// is not its first entry.
fn with_project_a() -> (Scenario, Board) {
    plans_board(&[(
        "projects/a/questions/q",
        body("2026-10-01T09:00:00Z", &json!({})),
    )])
}

/// The input that records "Ship it." in project `a` for `local_aa`.
fn deciding() -> Value {
    json!({
        "project": "a",
        "decision": "Ship it.",
        "why": "It is ready.",
        "session": "local_aa",
    })
}

/// A call with a project, a decision, a reason and a session writes one
/// valid decision under the project's decisions folder, decided at the time
/// the call ran, and says so in its line.
#[test]
fn a_decision_from_its_flags_is_written_under_its_project_and_decided_now() {
    let (scenario, board) = with_project_a();
    let before = utc_seconds(SystemTime::now());
    let output = decide(&board, &deciding());
    let after = utc_seconds(SystemTime::now());
    let path = output["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("/projects/a/decisions/"), "{path}");
    assert_eq!(output["line"], json!(format!("Recorded {path}.")));
    assert_eq!(output.get("warning"), None, "{output}");
    let record = stored(&scenario, &path);
    let decided = record["decided"].as_str().unwrap().to_owned();
    assert!(
        before.as_str() <= decided.as_str() && decided.as_str() <= after.as_str(),
        "{before} <= {decided} <= {after}"
    );
    assert_eq!(
        record,
        json!({
            "$type": "plans/decision/v1",
            "decision": "Ship it.",
            "why": "It is ready.",
            "by": {"session": "local_aa"},
            "decided": decided,
        })
    );
    let scope: board_model::LogicalPath = path.parse().unwrap();
    let checked = board.check(Some(&scope), None).unwrap();
    assert!(!checked.degraded(), "{:?}", checked.entries);
}

/// `decided` is the time the host gives, which here is the epoch.
#[test]
fn decided_is_the_hosts_time() {
    let (_scenario, board) = with_project_a();
    let mut host = host(Vec::new());
    host.board = Some(board);
    crate::decide::decide(&host, &deciding()).unwrap();
    let lines = host.puts.borrow();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (&lines[0][0]["new"], &lines[0][0]["record"]["decided"]),
        (
            &json!("/projects/a/decisions/"),
            &json!("1970-01-01T00:00:00Z")
        )
    );
}

/// `by` holds the session, the person and the program the call names, the
/// lists and `review: wanted` are kept, and `review` false leaves it out.
#[test]
fn the_author_lists_and_review_are_kept_and_review_false_leaves_it_out() {
    let (scenario, board) = with_project_a();
    let mut input = deciding();
    for (name, value) in [
        ("person", json!("ana")),
        ("program", json!("claude-code")),
        ("review", json!(true)),
        ("alternatives", json!(["Wait.", "Drop it."])),
        ("refs", json!(["/projects/a/questions/q"])),
        ("urls", json!(["https://example.com/pull/1"])),
    ] {
        input[name] = value;
    }
    let path = decide(&board, &input)["path"].as_str().unwrap().to_owned();
    let record = stored(&scenario, &path);
    assert_eq!(
        (
            &record["by"],
            &record["review"],
            &record["alternatives"],
            &record["refs"],
            &record["urls"]
        ),
        (
            &json!({"session": "local_aa", "person": "ana", "program": "claude-code"}),
            &json!("wanted"),
            &json!(["Wait.", "Drop it."]),
            &json!(["/projects/a/questions/q"]),
            &json!(["https://example.com/pull/1"]),
        )
    );
    input["review"] = json!(false);
    let path = decide(&board, &input)["path"].as_str().unwrap().to_owned();
    assert_eq!(stored(&scenario, &path).get("review"), None);
}

/// `decides` is written as the entry the decision decides, and a call
/// without it writes none; a `decides` that is no board path is refused by
/// the input schema.
#[test]
fn decides_is_written_when_given_and_a_value_that_is_no_board_path_is_refused() {
    let (scenario, board) = with_project_a();
    let mut input = deciding();
    input["decides"] = json!("/projects/a/questions/q");
    let path = decide(&board, &input)["path"].as_str().unwrap().to_owned();
    let record = stored(&scenario, &path);
    assert_eq!(record["decides"], json!("/projects/a/questions/q"));
    let scope: board_model::LogicalPath = path.parse().unwrap();
    let checked = board.check(Some(&scope), None).unwrap();
    assert!(!checked.degraded(), "{:?}", checked.entries);
    let path = decide(&board, &deciding())["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(stored(&scenario, &path).get("decides"), None);
    input["decides"] = json!("not a path");
    let refusal = refused(&board, &input);
    assert_eq!(refusal.code.as_str(), "board::plugin::bad_input");
}

/// A call that names neither a session nor a person is refused with the
/// decide's own code, whose hint names both flags, and nothing is written.
/// The twin that names only a person is written.
#[test]
fn a_call_with_no_session_or_person_is_refused_and_its_person_twin_is_written() {
    let (_scenario, board) = with_project_a();
    let mut input = deciding();
    input.as_object_mut().unwrap().remove("session");
    input["program"] = json!("codex");
    let refusal = refused(&board, &input);
    assert_eq!(refusal.code.as_str(), "plans::decide::no_author");
    let hint = refusal.hint.unwrap();
    assert!(
        hint.contains("--session") && hint.contains("--person"),
        "{hint}"
    );
    let written = board.query(
        "SELECT logical FROM entries WHERE logical LIKE '/projects/a/decisions/%'",
        10,
        None,
        None,
    );
    assert_eq!(written.unwrap().rows.len(), 0);
    input["person"] = json!("ana");
    let output = decide(&board, &input);
    assert!(
        output["path"]
            .as_str()
            .unwrap()
            .starts_with("/projects/a/decisions/")
    );
}

/// A call that leaves out the project, the decision or the reason, or
/// whose session is no `local_` id, is refused by the input schema with
/// Board's code, naming the property. The whole call is written.
#[test]
fn a_call_missing_a_required_flag_or_with_a_bad_session_is_refused_and_the_whole_one_is_written() {
    let (_scenario, board) = with_project_a();
    for name in ["project", "decision", "why"] {
        let mut input = deciding();
        input.as_object_mut().unwrap().remove(name);
        let refusal = refused(&board, &input);
        assert_eq!(refusal.code.as_str(), "board::plugin::bad_input", "{name}");
        assert!(
            refusal.message.contains(name),
            "{name}: {}",
            refusal.message
        );
    }
    let mut input = deciding();
    input["session"] = json!("aa");
    let refusal = refused(&board, &input);
    assert_eq!(refusal.code.as_str(), "board::plugin::bad_input");
    assert!(refusal.message.contains("session"), "{}", refusal.message);
    assert!(decide(&board, &deciding())["path"].is_string());
}

/// A decision in a project that holds nothing else is still written, with a
/// warning that names the three projects whose names are nearest. The twin
/// in a project that holds an entry has no warning.
#[test]
fn a_decision_in_a_new_project_warns_with_the_nearest_projects_and_its_twin_does_not() {
    let question = || body("2026-10-01T09:00:00Z", &json!({}));
    let (scenario, board) = plans_board(&[
        ("projects/board/questions/q", question()),
        ("projects/boards/questions/q", question()),
        ("projects/hoard/questions/q", question()),
        ("projects/crumbkit/questions/q", question()),
        ("projects/beard/questions/q", question()),
    ]);
    let mut input = deciding();
    input["project"] = json!("bord");
    let output = decide(&board, &input);
    let path = output["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("/projects/bord/decisions/"), "{path}");
    assert_eq!(stored(&scenario, &path)["decision"], json!("Ship it."));
    assert_eq!(
        output["warning"],
        json!({
            "code": "plans::decide::new_project",
            "message": "/projects/bord held nothing before this decision, so it starts a new project; the nearest projects are board, beard and boards.",
        })
    );
    input["project"] = json!("board");
    assert_eq!(decide(&board, &input).get("warning"), None);
}

/// A board with no project warns that it holds no other project.
#[test]
fn a_decision_in_a_board_with_no_project_warns_that_there_is_none() {
    let (_scenario, board) = plans_board(&[]);
    let output = decide(&board, &deciding());
    assert_eq!(
        output["warning"]["message"],
        json!(
            "/projects/a held nothing before this decision, so it starts a new project; the board holds no other project."
        )
    );
}

/// The same call twice writes two decisions, each with its own id.
#[test]
fn a_repeat_of_the_same_call_writes_a_second_decision() {
    let (_scenario, board) = with_project_a();
    let first = decide(&board, &deciding());
    let second = decide(&board, &deciding());
    assert_ne!(first["path"], second["path"]);
}

/// A put the board refuses, here into a board whose schema for decisions
/// refuses every one, is reported with the plans code and Board's code, as
/// `plans.ask` reports it, and nothing is written. The twin, whose board
/// keeps the shipped schema, is written.
#[test]
fn a_put_the_board_refuses_is_reported_with_boards_code_and_its_twin_is_written() {
    for broken in [true, false] {
        let (scenario, board) = with_project_a();
        let mut host = host(Vec::new());
        host.board = Some(board);
        let scenario = Rc::new(scenario);
        if broken {
            let there = Rc::clone(&scenario);
            *host.between.borrow_mut() = Some(Box::new(move || {
                there.write(
                    ".board/types/plans/decision/v1.schema.json",
                    Item::File(r#"{"not": {}}"#),
                );
            }));
        }
        let result = crate::decide::decide(&host, &deciding());
        let written = std::fs::read_dir(scenario.workdir().join("projects/a/decisions"));
        if broken {
            let refusal = result.unwrap_err();
            assert_eq!(refusal.code, "plans::board::refused");
            assert!(refusal.fields["code"].is_string(), "{:?}", refusal.fields);
            assert!(written.is_err(), "nothing is written");
        } else {
            assert!(result.unwrap()["path"].is_string());
            assert_eq!(written.unwrap().count(), 1);
        }
    }
}

//! `changes.owed` and the warning of `changes.open` through the real index,
//! for each way a done change can owe nothing: its flow's last phase has
//! only checks that block, or each check's `when` fails on the change's
//! route or its plan's answers. A `when` the answers cannot settle, such as
//! one on the plan's text, is judged as the gate judges it.

use super::*;

/// The done change each test writes.
const CHANGE: &str = "/changes/a";

/// A flow called `own` whose one phase, `land`, has `checks`.
fn own(checks: &Value) -> String {
    json!({
        "$type": "changes/flow/v1",
        "name": "own",
        "summary": "A flow for a test.",
        "stop": {"through": "land"},
        "phases": [{"name": "land", "agent": "lead", "instruction": "Land it.", "checks": checks}],
    })
    .to_string()
}

/// A check that does not block and is missing while the change records
/// no head, with `when`.
fn unheaded(when: &Value) -> Value {
    let mut check = json!({"id": "headed", "kind": "field", "field": "head", "blocks": false});
    if !when.is_null() {
        check["when"] = when.clone();
    }
    check
}

/// The fixture board holding a done change at [`CHANGE`] on the flow
/// `own` with `checks`, touched today, with `fields`, whose plan has
/// `text` and, when given, `answers`.
fn done(checks: &Value, fields: &Value, text: &str, answers: Option<&Value>) -> Changes {
    let board = Changes::new();
    board.write("flows/own.json", &own(checks));
    board.write("changes/a/plans/1.md", text);
    let mut plan = json!({"text": "/changes/a/plans/1.md", "uses": []});
    if let Some(answers) = answers {
        plan["answers"] = answers.clone();
    }
    board.write("changes/a/plans/1.json", &typed("changes/plan/v1", &plan));
    let change = json!({
        "title": "A done change",
        "flow": "/flows/own",
        "phase": "done",
        "touched": today(),
        "plan": "/changes/a/plans/1",
    });
    board.write(
        "changes/a.json",
        &typed(
            "changes/change/v2",
            &serde_json::from_str(&record(change, fields)).unwrap(),
        ),
    );
    assert_eq!(board.code(CHANGE), None);
    board
}

/// The paths `changes.owed` lists, each with the detail of each check it
/// owes, and the code of the warning `changes.open` gives, if any.
fn owing(board: &Changes) -> (Vec<(String, Vec<String>)>, Option<String>) {
    let owed = board.call("owed", &json!({}));
    let listed = (owed["changes"].as_array().unwrap().iter())
        .map(|each| {
            let checks = (each["checks"].as_array().unwrap().iter())
                .map(|check| check["detail"].as_str().unwrap().to_owned())
                .collect();
            (each["path"].as_str().unwrap().to_owned(), checks)
        })
        .collect();
    assert_eq!(owed["total"], owed["changes"].as_array().unwrap().len());
    let open = board.call("open", &json!({}));
    let warned =
        (open.get("warnings")).map(|warnings| warnings[0]["code"].as_str().unwrap().to_owned());
    (listed, warned)
}

/// What [`owing`] gives for a board where [`CHANGE`] owes its one check,
/// missing with `detail`.
fn owes(detail: &str) -> (Vec<(String, Vec<String>)>, Option<String>) {
    (
        vec![(CHANGE.to_owned(), vec![detail.to_owned()])],
        Some("changes::owed::rechecks".to_owned()),
    )
}

const NOTHING: (Vec<(String, Vec<String>)>, Option<String>) = (Vec::new(), None);
const FIELD: &str = "the change records no head commit; attach one with --head";
const UNANSWERED: &str =
    "the plan gives no typed answer for data (none or stored), which this check reads";
const STORED: &str = "# A\n\n## Data\n\nOne record is stored.\n";

#[test]
fn a_done_change_whose_last_phase_only_blocks_owes_nothing_and_its_twin_owes_its_check() {
    let blocking = json!([{"id": "headed", "kind": "field", "field": "head"}]);
    let board = done(&blocking, &json!({}), STORED, None);
    assert_eq!(owing(&board), NOTHING);
    let board = done(&json!([unheaded(&Value::Null)]), &json!({}), STORED, None);
    assert_eq!(owing(&board), owes(FIELD));
}

#[test]
fn a_done_change_whose_answers_fail_the_when_owes_nothing_and_its_twin_owes_its_check() {
    let checks = json!([unheaded(&json!({"answers": {"data": "stored"}}))]);
    let none = json!({"ux": "none", "data": "none", "lifecycle": "none"});
    let stored = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    assert_eq!(
        owing(&done(&checks, &json!({}), STORED, Some(&none))),
        NOTHING
    );
    assert_eq!(
        owing(&done(&checks, &json!({}), STORED, Some(&stored))),
        owes(FIELD)
    );
    let unless = json!([unheaded(&json!({"answers_not": {"data": "none"}}))]);
    assert_eq!(
        owing(&done(&unless, &json!({}), STORED, Some(&none))),
        NOTHING
    );
    assert_eq!(
        owing(&done(&unless, &json!({}), STORED, Some(&stored))),
        owes(FIELD)
    );
}

#[test]
fn a_done_change_whose_route_fails_the_when_owes_nothing_and_its_twin_owes_its_check() {
    let checks = json!([unheaded(&json!({"route": ["owner"]}))]);
    let unowned = json!({"route": "unowned"});
    assert_eq!(owing(&done(&checks, &unowned, STORED, None)), NOTHING);
    let owner = json!({"route": "owner"});
    assert_eq!(owing(&done(&checks, &owner, STORED, None)), owes(FIELD));
}

#[test]
fn a_plan_that_gives_no_answer_the_when_reads_leaves_the_check_owed_as_the_gate_says() {
    let checks = json!([unheaded(&json!({"answers": {"data": "stored"}}))]);
    let board = done(&checks, &json!({}), STORED, None);
    assert_eq!(owing(&board), owes(UNANSWERED));
    // A plan record Board does not hold gives no answers either.
    board.delete("changes/a/plans/1.json");
    assert_eq!(owing(&board), owes(UNANSWERED));
    // Its twin whose plan answers owes the check itself.
    let stored = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    assert_eq!(
        owing(&done(&checks, &json!({}), STORED, Some(&stored))),
        owes(FIELD)
    );
}

#[test]
fn a_when_on_the_plans_text_is_judged_on_the_text_whatever_the_answers() {
    let checks = json!([unheaded(&json!({
        "section": {"Data": "stored"},
        "answers": {"data": "stored"},
    }))]);
    let stored = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    let board = done(&checks, &json!({}), STORED, Some(&stored));
    assert_eq!(owing(&board), owes(FIELD));
    let headless = "# A\n\n## Data\n\nNothing.\n";
    let board = done(&checks, &json!({}), headless, Some(&stored));
    assert_eq!(owing(&board), NOTHING);
}

#[test]
fn an_owed_check_whose_remedy_names_the_board_gives_the_boards_root() {
    let mut check = unheaded(&Value::Null);
    check["remedy"] = json!("Attach a head in {board}.");
    let board = done(&json!([check]), &json!({}), STORED, None);
    let owed = board.call("owed", &json!({}));
    let remedy = owed["changes"][0]["checks"][0]["remedy"].as_str().unwrap();
    let root = board.root().file_name().unwrap().to_str().unwrap();
    assert!(remedy.starts_with("Attach a head in /"), "{remedy}");
    assert!(remedy.ends_with(&format!("{root}.")), "{remedy}");
}

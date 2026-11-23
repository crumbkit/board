//! The land gate's `rechecks` check through the real index: it reads the
//! points of an attached understanding check as stored, does not block,
//! and `owed` lists a done change while it is missing.

use super::*;

const CHANGE: &str = "/changes/t";
const RECORD: &str = "/changes/t/understanding/data";

/// A change in land with stored data whose plan attaches the understanding
/// check at [`RECORD`], holding rechecks with the given `points`, and a
/// decision that signs it off.
fn built(points: &[Option<&str>]) -> Changes {
    let board = Changes::new();
    board.write(
        "decisions/data.json",
        &typed(
            "plans/decision/v1",
            &json!({"decision": "Agreed.", "refs": [RECORD], "by": {"person": PERSON}}),
        ),
    );
    board.change_at(
        "changes/t.json",
        "land",
        &json!({"route": "owner", "records": [board.approved(), RECORD, "/decisions/data"],
                "pull_request": "https://github.com/o/r/pull/7"}),
    );
    board.write("changes/t/plans/1.md", &format!("{PLAN}{TESTED}"));
    let mut plan = board.read("changes/t/plans/1.json");
    plan["answers"] = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    board.write("changes/t/plans/1.json", &plan.to_string());
    let rechecks: Vec<Value> = (points.iter())
        .map(|point| {
            let mut recheck = json!({
                "question": "Q?", "answer": "A.", "design": "A.",
                "kind": "edge", "matches": true,
            });
            if let Some(point) = point {
                recheck["point"] = json!(point);
            }
            recheck
        })
        .collect();
    board.write(
        "changes/t/understanding/data.json",
        &typed(
            "changes/understanding/v1",
            &json!({
                "change": CHANGE,
                "subject": "data",
                "questions": [{"question": "Q?", "answer": "A.", "design": "A."}],
                "rechecks": rechecks,
            }),
        ),
    );
    board
}

/// The `rechecks` entry of `changes.next` for [`CHANGE`].
fn rechecks_gate(board: &Changes) -> Value {
    let output = board.call("next", &json!({"path": CHANGE}));
    (output["gate"].as_array().unwrap().iter())
        .find(|each| each["check"] == "rechecks")
        .unwrap()
        .clone()
}

#[test]
fn a_stored_understanding_check_with_two_points_passes_the_rechecks_check() {
    let board = built(&[Some("first-point"), Some("second-point")]);
    assert_eq!(board.code(RECORD), None);
    let gate = rechecks_gate(&board);
    assert_eq!(gate["status"], "pass", "{gate}");
}

#[test]
fn a_stored_recheck_with_no_point_is_degraded_and_the_gate_says_so_with_its_remedy() {
    let board = built(&[None, None]);
    assert_eq!(
        board.code(RECORD).as_deref(),
        Some("board::record::invalid")
    );
    let gate = rechecks_gate(&board);
    assert_eq!(gate["status"], "missing", "{gate}");
    assert!(
        gate["detail"]
            .as_str()
            .unwrap()
            .contains("is degraded with"),
        "{gate}"
    );
    let remedy = gate["remedy"].as_str().unwrap();
    assert!(remedy.contains("Give each recheck a `point`"), "{remedy}");
}

/// The sections the land gate needs.
const TESTED: &str =
    "\n## What the tests prove\n\nIt stops.\n\n## What the tests do not prove\n\nRetries.\n";

/// Decision 1306Z: a land gate whose only missing check is the rechecks
/// passes, as `github.merge` reads it, so the change lands; `owed` then
/// lists it and `open` warns until two points match.
#[test]
fn a_land_gate_missing_only_rechecks_passes_and_owed_lists_the_done_change_until_they_match() {
    let board = built(&[Some("first-point")]);
    let next = board.call("next", &json!({"path": CHANGE}));
    let gate = rechecks_gate(&board);
    assert_eq!(
        (&gate["status"], &gate["blocks"]),
        (&json!("missing"), &json!(false))
    );
    let blocking: Vec<&Value> = (next["gate"].as_array().unwrap().iter())
        .filter(|each| each["status"] == "missing" && each["blocks"] == true)
        .collect();
    assert_eq!(blocking, Vec::<&Value>::new(), "{next}");
    assert_eq!(
        next["instruction"],
        "The gate passes: run `board changes advance --path /changes/t`."
    );
    let moved = board.call("advance", &json!({"path": CHANGE}));
    assert_eq!(moved["to"], "done");
    let owed = board.call("owed", &json!({}));
    assert_eq!(owed["total"], 1, "{owed}");
    assert_eq!(owed["changes"][0]["path"], CHANGE);
    assert_eq!(owed["changes"][0]["checks"][0]["check"], "rechecks");
    let open = board.call("open", &json!({}));
    assert_eq!(
        open["warnings"][0]["code"], "changes::owed::rechecks",
        "{open}"
    );
    // Its twin with two matching points owes nothing.
    let mut record = board.read("changes/t/understanding/data.json");
    record["rechecks"].as_array_mut().unwrap().push(json!({
        "question": "Q2?", "answer": "A.", "design": "A.", "kind": "edge",
        "matches": true, "point": "second-point",
    }));
    board.write("changes/t/understanding/data.json", &record.to_string());
    let owed = board.call("owed", &json!({}));
    assert_eq!(owed["total"], 0, "{owed}");
    let open = board.call("open", &json!({}));
    assert_eq!(open.get("warnings"), None, "{open}");
}

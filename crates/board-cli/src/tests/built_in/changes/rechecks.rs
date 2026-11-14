//! The build gate's `rechecks` check through the real index: it reads the
//! points of an attached understanding check as stored.

use super::*;

const CHANGE: &str = "/changes/t";
const RECORD: &str = "/changes/t/understanding/data";

/// A change in build with stored data whose plan attaches the understanding
/// check at [`RECORD`], holding rechecks with the given `points`, and a
/// decision that signs it off.
fn built(points: &[Option<&str>]) -> Changes {
    let board = Changes::new();
    board.write(
        "decisions/data.json",
        &typed(
            "plans/decision/v1",
            &json!({"decision": "Agreed.", "refs": [RECORD]}),
        ),
    );
    board.change_at(
        "changes/t.json",
        "build",
        &json!({"route": "owner", "records": [board.approved(), RECORD, "/decisions/data"]}),
    );
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

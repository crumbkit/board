//! `ask` files a question only when no open question in its project asks
//! the same words and no answered one does.

use board_ops::{Board, Plugins};
use board_plugin::Target;
use serde_json::{Value, json};

use crate::open::tests::{answer_naming, body, plans_board};

/// What `plans.ask` returns for `input` on `board`.
fn ask(board: &Board, input: &Value) -> Value {
    let target: Target = "plans.ask".parse().unwrap();
    let input = input.to_string();
    board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap()
        .output
}

/// The open questions `plans.open` lists, by path.
fn open(board: &Board) -> Vec<Value> {
    let target: Target = "plans.open".parse().unwrap();
    let output = board
        .call(&target, b"{}", &Plugins::default())
        .unwrap()
        .output;
    output["questions"].as_array().unwrap().clone()
}

/// The input that asks `words` in project `a` for `session`.
fn asking(words: &str, session: &str) -> Value {
    json!({
        "project": "a",
        "question": {
            "question": words,
            "by": {"session": session},
            "options": [{"id": "y", "text": "Yes."}, {"id": "n", "text": "No."}],
        },
    })
}

#[test]
fn a_new_question_is_filed_open_in_its_project() {
    let (_scenario, board) = plans_board(&[]);
    let output = ask(&board, &asking("Ship it?", "local_aa"));
    assert_eq!(output["outcome"], json!("filed"));
    let path = output["path"].as_str().unwrap();
    assert!(path.starts_with("/projects/a/questions/"), "{path}");
    let listed = open(&board);
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            &listed[0]["path"],
            &listed[0]["question"],
            &listed[0]["asked"]
        ),
        (
            &json!(path),
            &json!("Ship it?"),
            &json!("1970-01-01T00:00:00Z")
        )
    );
}

/// A question whose words an open question in its project already asks is
/// not filed. Its asker is added to that question's `also_by`, so the answer
/// reaches both. The twin with other words is filed.
#[test]
fn a_repeat_of_an_open_question_is_merged_into_it_and_other_words_are_filed() {
    let (_scenario, board) = plans_board(&[(
        "projects/a/questions/first",
        body(
            "2026-10-01T09:00:00Z",
            &json!({"question": "Ship it?", "by": {"session": "local_aa"}}),
        ),
    )]);
    let output = ask(&board, &asking("ship it", "local_bb"));
    assert_eq!(
        (&output["outcome"], &output["path"]),
        (&json!("merged"), &json!("/projects/a/questions/first"))
    );
    let listed = open(&board);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["also_asked"], json!(["local_bb"]));
    let output = ask(&board, &asking("Ship it today?", "local_bb"));
    assert_eq!(output["outcome"], json!("filed"));
    assert_eq!(open(&board).len(), 2);
}

/// The asker of the open question, or one already merged into it, asking
/// again is merged and the question is not written again.
#[test]
fn the_same_asker_asking_again_changes_nothing() {
    let (_scenario, board) = plans_board(&[(
        "projects/a/questions/first",
        body(
            "2026-10-01T09:00:00Z",
            &json!({"question": "Ship it?", "by": {"session": "local_aa"}}),
        ),
    )]);
    let output = ask(&board, &asking("Ship it?", "local_aa"));
    assert_eq!(output["outcome"], json!("merged"));
    assert_eq!(open(&board)[0]["also_asked"], json!([]));
    ask(&board, &asking("Ship it?", "local_bb"));
    ask(&board, &asking("Ship it?", "local_bb"));
    assert_eq!(open(&board)[0]["also_asked"], json!(["local_bb"]));
}

/// A question whose words a question answered in its project asked gets
/// that answer back, with the decisions that name the question or its
/// answer, and nothing is filed. The twin in another project is filed.
#[test]
fn a_question_already_answered_returns_the_answer_and_its_decisions_and_is_not_filed() {
    let decision = Value::String(
        json!({
            "$type": "plans/decision/v1",
            "decision": "Ship on Fridays.",
            "why": "The owner said so.",
            "by": {"person": "pat"},
            "decided": "2026-10-02T09:00:00Z",
            "refs": ["/projects/a/answers/before"],
        })
        .to_string(),
    );
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/before",
            body("2026-10-01T09:00:00Z", &json!({"question": "Ship it?"})),
        ),
        (
            "projects/a/answers/before",
            answer_naming("/projects/a/questions/before"),
        ),
        ("projects/a/decisions/ship", decision),
    ]);
    let output = ask(&board, &asking("Ship it?", "local_bb"));
    assert_eq!(
        output,
        json!({
            "outcome": "answered",
            "path": "/projects/a/questions/before",
            "answer": {"path": "/projects/a/answers/before", "choice": null, "words": "Done."},
            "decisions": ["/projects/a/decisions/ship"],
            "line": "The owner already answered this at /projects/a/answers/before: Done."
        })
    );
    assert_eq!(open(&board).len(), 0);
    let mut elsewhere = asking("Ship it?", "local_bb");
    elsewhere["project"] = json!("b");
    assert_eq!(ask(&board, &elsewhere)["outcome"], json!("filed"));
}

/// Board refuses a project that is not one path segment with its input
/// code, and admits the twin that is one.
#[test]
fn board_refuses_a_project_that_is_no_slug_and_admits_its_twin() {
    let (_scenario, board) = plans_board(&[]);
    let target: Target = "plans.ask".parse().unwrap();
    let mut input = asking("Ship it?", "local_aa");
    input["project"] = json!("a/b");
    let refused = board
        .call(&target, input.to_string().as_bytes(), &Plugins::default())
        .unwrap_err();
    assert_eq!(refused.refusal().code.as_str(), "board::plugin::bad_input");
    input["project"] = json!("a-b");
    assert!(
        board
            .call(&target, input.to_string().as_bytes(), &Plugins::default())
            .is_ok()
    );
}

/// A question its schema refuses, here one whose asker names no session, is
/// refused with Board's put code and nothing is filed. The twin that names
/// one is filed.
#[test]
fn a_question_its_schema_refuses_is_not_filed_and_its_twin_is() {
    let (_scenario, board) = plans_board(&[]);
    let target: Target = "plans.ask".parse().unwrap();
    let mut input = asking("Ship it?", "local_aa");
    input["question"]["by"] = json!({"program": "codex"});
    let refused = board
        .call(&target, input.to_string().as_bytes(), &Plugins::default())
        .unwrap_err();
    let refusal = refused.refusal();
    assert_eq!(refusal.code.as_str(), "plans::board::refused");
    assert_eq!(open(&board).len(), 0);
    input["question"]["by"] = json!({"program": "codex", "session": "local_aa"});
    assert_eq!(
        board
            .call(&target, input.to_string().as_bytes(), &Plugins::default())
            .unwrap()
            .output["outcome"],
        json!("filed")
    );
}

//! `ask` files a question only when no open question in its project asks
//! the same words and no answered one does.

use std::rc::Rc;
use std::time::SystemTime;

use board_model::utc_seconds;
use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::Item;
use serde_json::{Value, json};

use crate::open::tests::{answer_naming, body, host, plans_board};

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

/// A question nobody asked yet is filed open, asked at the time the call
/// ran, which `Board::call` gives the host as its settings' `now`.
#[test]
fn a_new_question_is_filed_open_in_its_project() {
    let (_scenario, board) = plans_board(&[]);
    let before = utc_seconds(SystemTime::now());
    let output = ask(&board, &asking("Ship it?", "local_aa"));
    let after = utc_seconds(SystemTime::now());
    assert_eq!(output["outcome"], json!("filed"));
    let path = output["path"].as_str().unwrap();
    assert!(path.starts_with("/projects/a/questions/"), "{path}");
    let listed = open(&board);
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (&listed[0]["path"], &listed[0]["question"]),
        (&json!(path), &json!("Ship it?"))
    );
    let asked = listed[0]["asked"].as_str().unwrap();
    assert!(
        before.as_str() <= asked && asked <= after.as_str(),
        "{before} <= {asked} <= {after}"
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

/// A question its schema refuses, here one whose asker names neither a
/// session nor a person, is refused with Board's put code and nothing is
/// filed. The twin that names a session is filed, and so is the twin that
/// names only a person.
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
    let mut person = asking("Ship it today?", "local_aa");
    person["question"]["by"] = json!({"person": "ana"});
    assert_eq!(ask(&board, &person)["outcome"], json!("filed"));
}

/// The input that asks `words` in project `a` for the person `person`, who
/// has no session.
fn asking_as(words: &str, person: &str) -> Value {
    let mut input = asking(words, "local_aa");
    input["question"]["by"] = json!({"person": person});
    input
}

/// A person who asks the same words twice is merged the second time, as a
/// session is, and the question gains no asker. Another person asking them
/// is merged into the question's `also_by`.
#[test]
fn a_person_asking_again_is_merged_and_another_person_joins_also_by() {
    let (scenario, board) = plans_board(&[]);
    let filed = ask(&board, &asking_as("Ship it?", "ana"));
    assert_eq!(filed["outcome"], json!("filed"));
    let path = filed["path"].as_str().unwrap().to_owned();
    let again = ask(&board, &asking_as("ship it", "ana"));
    assert_eq!(
        (&again["outcome"], &again["path"]),
        (&json!("merged"), &json!(path))
    );
    let stored = || -> Value {
        let file = scenario.workdir().join(format!("{}.json", &path[1..]));
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    };
    assert_eq!(stored().get("also_by"), None);
    let other = ask(&board, &asking_as("Ship it?", "bo"));
    assert_eq!(other["outcome"], json!("merged"));
    assert_eq!(stored()["also_by"], json!([{"person": "bo"}]));
    ask(&board, &asking_as("Ship it?", "bo"));
    assert_eq!(stored()["also_by"], json!([{"person": "bo"}]));
    let listed = open(&board);
    assert_eq!(
        (&listed[0]["person"], &listed[0]["also_asked"]),
        (&json!("ana"), &json!([]))
    );
}

/// The open question "Ship it?" at /projects/a/questions/first, asked by
/// `local_aa`.
fn first() -> (&'static str, Value) {
    (
        "projects/a/questions/first",
        body(
            "2026-10-01T09:00:00Z",
            &json!({"question": "Ship it?", "by": {"session": "local_aa"}}),
        ),
    )
}

/// An asker whose `by` names no session is refused with the code filing
/// gives, before the open question it repeats is read or written, so the
/// answer is never owed to a session nobody can name. The twin, whose `by`
/// names a session, is merged.
#[test]
fn a_merge_whose_asker_names_no_session_is_refused_and_its_twin_is_merged() {
    let (_scenario, board) = plans_board(&[first()]);
    let target: Target = "plans.ask".parse().unwrap();
    let mut input = asking("Ship it?", "local_bb");
    input["question"]["by"] = json!({"program": "codex"});
    let refused = board
        .call(&target, input.to_string().as_bytes(), &Plugins::default())
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code.as_str(), "plans::board::refused");
    assert_eq!(open(&board)[0]["also_asked"], json!([]));
    input["question"]["by"] = json!({"program": "codex", "session": "local_bb"});
    let output = board
        .call(&target, input.to_string().as_bytes(), &Plugins::default())
        .unwrap()
        .output;
    assert_eq!(output["outcome"], json!("merged"));
    assert_eq!(open(&board)[0]["also_asked"], json!(["local_bb"]));
}

#[test]
fn board_refuses_an_unknown_input_field_and_admits_its_twin() {
    let (_scenario, board) = plans_board(&[]);
    let target: Target = "plans.ask".parse().unwrap();
    let mut input = asking("Ship it?", "local_aa");
    input["urgent"] = json!(true);
    let refused = board
        .call(&target, input.to_string().as_bytes(), &Plugins::default())
        .unwrap_err();
    assert_eq!(refused.refusal().code.as_str(), "board::plugin::bad_input");
    input.as_object_mut().unwrap().remove("urgent");
    assert_eq!(ask(&board, &input)["outcome"], json!("filed"));
}

/// The open question changing between `plans.ask`'s read and its merge put
/// is refused with Board's code for a stale write, and the question keeps
/// the change and gains no asker. Answered or withdrawn before the read,
/// it is refused with the ask's own code and a hint to ask again. The twin,
/// unchanged, is merged.
#[test]
fn a_question_that_changes_before_the_merge_is_refused_and_its_unchanged_twin_is_merged() {
    let noted = body(
        "2026-10-01T09:00:00Z",
        &json!({"question": "Ship it?", "by": {"session": "local_aa"}, "note": "Changed."}),
    );
    let withdrawn = body(
        "2026-10-01T09:00:00Z",
        &json!({"question": "Ship it?", "by": {"session": "local_aa"},
                "status": "withdrawn", "withdrawn_reason": "Not needed."}),
    );
    let answer = answer_naming("/projects/a/questions/first");
    let cases = [
        (
            "between",
            Some(("projects/a/questions/first", noted)),
            Some(("plans::board::refused", Some("board::put::stale"))),
        ),
        (
            "before_read",
            Some(("projects/a/answers/first", answer)),
            Some(("plans::ask::already_answered", None)),
        ),
        (
            "before_read",
            Some(("projects/a/questions/first", withdrawn)),
            Some(("plans::ask::not_open", None)),
        ),
        ("between", None, None),
    ];
    for (hook, change, refusal) in cases {
        let (scenario, board) = plans_board(&[first()]);
        let mut host = host(Vec::new());
        host.board = Some(board);
        let scenario = Rc::new(scenario);
        if let Some((path, record)) = change {
            let there = Rc::clone(&scenario);
            let write: Box<dyn FnOnce()> = Box::new(move || {
                there.write(
                    &format!("{path}.json"),
                    Item::File(record.as_str().unwrap()),
                );
            });
            match hook {
                "between" => *host.between.borrow_mut() = Some(write),
                _ => *host.before_read.borrow_mut() = Some(write),
            }
        }
        let result = crate::ask::ask(&host, &asking("Ship it?", "local_bb"));
        let Some((code, board_code)) = refusal else {
            assert_eq!(result.unwrap()["outcome"], json!("merged"));
            let board = host.board.as_ref().unwrap();
            assert_eq!(open(board)[0]["also_asked"], json!(["local_bb"]));
            continue;
        };
        let refused = result.unwrap_err();
        assert_eq!(refused.code, code);
        match board_code {
            Some(board_code) => assert_eq!(refused.fields["code"], json!(board_code)),
            None => assert_eq!(
                refused.hint.as_deref(),
                Some("run `board plans ask` again; it reads each question afresh")
            ),
        }
        let stored =
            std::fs::read_to_string(scenario.workdir().join("projects/a/questions/first.json"));
        let stored: Value = serde_json::from_str(&stored.unwrap()).unwrap();
        assert_eq!(stored.get("also_by"), None, "{code} adds no asker");
    }
}

/// When two open questions in a project ask the same words, a session that
/// asked either is not added to the other. Here `local_bb` asked `second`,
/// which `plans.open` lists after `first`. The twin, a new session, is
/// merged into `first`, the one listed first.
#[test]
fn a_session_that_asked_one_of_two_repeats_is_not_added_to_the_other() {
    let second = body(
        "2026-10-02T09:00:00Z",
        &json!({"question": "Ship it?", "by": {"session": "local_bb"}}),
    );
    let (_scenario, board) = plans_board(&[first(), ("projects/a/questions/second", second)]);
    let also_asked = |board: &Board| -> Vec<Value> {
        open(board)
            .iter()
            .map(|each| each["also_asked"].clone())
            .collect()
    };
    let output = ask(&board, &asking("Ship it?", "local_bb"));
    assert_eq!(
        (&output["outcome"], &output["path"]),
        (&json!("merged"), &json!("/projects/a/questions/second"))
    );
    assert_eq!(also_asked(&board), [json!([]), json!([])]);
    let output = ask(&board, &asking("Ship it?", "local_cc"));
    assert_eq!(output["path"], json!("/projects/a/questions/first"));
    assert_eq!(also_asked(&board), [json!(["local_cc"]), json!([])]);
}

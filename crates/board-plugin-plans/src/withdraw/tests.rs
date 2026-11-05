//! `withdraw` marks an open question withdrawn with its reason in one put,
//! or refuses and says why not.

use board_ops::Plugins;
use board_plugin::Target;
use serde_json::json;

use super::*;
use crate::open::tests::{Answering, body, host, plans_board};
use crate::question::question_sql;

const QUESTION: &str = "/projects/a/questions/q";
const BLOB: &str = "4b825dc642cb6eb9a060e54bf8d69288fbe4904d";

/// The rows `question_sql` returns for the question at `QUESTION` with
/// `fields`, of `kind`, and an answer at each of `answers`.
fn rows(kind: &str, fields: &Value, answers: &[&str]) -> Vec<Vec<Value>> {
    let asked = vec![
        json!("question"),
        json!(QUESTION),
        json!(kind),
        json!(BLOB),
        body("2026-10-01T09:00:00Z", fields),
    ];
    let answered = answers.iter().map(|path| {
        vec![
            json!("answer"),
            json!(path),
            json!("plans/answer/v1"),
            json!(BLOB),
            Value::Null,
        ]
    });
    std::iter::once(asked).chain(answered).collect()
}

fn on(rows: Vec<Vec<Value>>) -> Answering {
    host(rows)
}

#[test]
fn a_withdrawal_is_one_put_of_the_question_withdrawn_with_its_reason() {
    let host = on(rows("plans/question/v1", &json!({}), &[]));
    let input = json!({"path": QUESTION, "reason": "The release shipped without it."});
    let output = withdraw(&host, &input).unwrap();
    assert_eq!(
        output,
        json!({"question": QUESTION, "reason": "The release shipped without it."})
    );
    assert_eq!(
        host.asked.take(),
        [(question_sql(QUESTION), board_ops::Limit::ALL)]
    );
    let mut withdrawn: Value =
        serde_json::from_str(body("2026-10-01T09:00:00Z", &json!({})).as_str().unwrap()).unwrap();
    withdrawn["status"] = json!("withdrawn");
    withdrawn["withdrawn_reason"] = json!("The release shipped without it.");
    assert_eq!(
        host.puts.take(),
        [vec![
            json!({"path": QUESTION, "expect": BLOB, "record": withdrawn})
        ]]
    );
}

/// Each refusal beside its twin, which differs in one thing and is
/// withdrawn: the path, the question's type, its fields and its answers.
struct Case {
    code: &'static str,
    refused: (&'static str, &'static str, Value, &'static [&'static str]),
    admitted: (&'static str, &'static str, Value, &'static [&'static str]),
    names: &'static str,
}

fn cases() -> Vec<Case> {
    let question = "plans/question/v1";
    vec![
        Case {
            code: "plans::withdraw::not_a_question_path",
            refused: ("/projects/a/answers/q", question, json!({}), &[]),
            admitted: (QUESTION, question, json!({}), &[]),
            names: "/projects/a/answers/q",
        },
        Case {
            code: "plans::withdraw::no_question",
            refused: (QUESTION, "plans/answer/v1", json!({}), &[]),
            admitted: (QUESTION, question, json!({}), &[]),
            names: "plans/answer/v1",
        },
        Case {
            code: "plans::withdraw::already_answered",
            refused: (QUESTION, question, json!({}), &["/projects/a/answers/q"]),
            admitted: (QUESTION, question, json!({}), &[]),
            names: "/projects/a/answers/q",
        },
        Case {
            code: "plans::withdraw::not_open",
            refused: (QUESTION, question, json!({"status": "withdrawn"}), &[]),
            admitted: (QUESTION, question, json!({"status": "open"}), &[]),
            names: "withdrawn",
        },
    ]
}

#[test]
fn each_refusal_has_its_own_code_and_says_what_was_wrong_and_its_twin_is_withdrawn() {
    for Case {
        code,
        refused,
        admitted,
        names,
    } in cases()
    {
        let run = |(path, kind, fields, answers): &(&str, &str, Value, &[&str])| {
            let host = on(rows(kind, fields, answers));
            let input = json!({"path": path, "reason": "Stale."});
            (withdraw(&host, &input), host.puts.take().len())
        };
        let (result, puts) = run(&refused);
        let error = result.unwrap_err();
        assert_eq!(error.code, code, "{}", error.message);
        assert!(error.message.contains(names), "{code}: {}", error.message);
        assert!(error.hint.is_some(), "{code} suggests what to try");
        assert_eq!(puts, 0, "{code} writes nothing");
        let (result, puts) = run(&admitted);
        assert!(
            result.is_ok(),
            "{code}'s twin: {:?}",
            result.err().map(|e| e.message)
        );
        assert_eq!(puts, 1, "{code}'s twin writes");
    }
}

#[test]
fn board_refuses_a_withdrawal_without_a_reason_and_withdraws_one_with_a_reason() {
    let (_scenario, board) = plans_board(&[(
        "projects/a/questions/q",
        body("2026-10-01T09:00:00Z", &json!({})),
    )]);
    let target: Target = "plans.withdraw".parse().unwrap();
    for input in [
        format!(r#"{{"path": "{QUESTION}"}}"#),
        format!(r#"{{"path": "{QUESTION}", "reason": ""}}"#),
    ] {
        let refused = board
            .call(&target, input.as_bytes(), &Plugins::default())
            .unwrap_err()
            .refusal();
        assert_eq!(refused.code.as_str(), "board::plugin::bad_input", "{input}");
    }
    let input = format!(r#"{{"path": "{QUESTION}", "reason": "Stale."}}"#);
    assert!(
        board
            .call(&target, input.as_bytes(), &Plugins::default())
            .is_ok()
    );
}

#[test]
fn board_calls_plans_withdraw_in_its_own_process_and_open_stops_listing_the_question() {
    let (scenario, board) = plans_board(&[(
        "projects/a/questions/q",
        body("2026-10-01T09:00:00Z", &json!({})),
    )]);
    let open: Target = "plans.open".parse().unwrap();
    let listed = |board: &board_ops::Board| {
        let report = board.call(&open, b"{}", &Plugins::default()).unwrap();
        report.output["total"].as_u64().unwrap()
    };
    assert_eq!(listed(&board), 1);
    let target: Target = "plans.withdraw".parse().unwrap();
    let input = format!(r#"{{"path": "{QUESTION}", "reason": "Stale."}}"#);
    let report = board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap();
    assert_eq!(report.output["question"], QUESTION);
    let text = std::fs::read_to_string(scenario.workdir().join("projects/a/questions/q.json"));
    let stored: Value = serde_json::from_str(&text.unwrap()).unwrap();
    assert_eq!(
        (&stored["status"], &stored["withdrawn_reason"]),
        (&json!("withdrawn"), &json!("Stale."))
    );
    let mut board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&crate::Plans]);
    assert_eq!(listed(&board), 0);
    let refused = board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code.as_str(), "plans::withdraw::not_open");
}

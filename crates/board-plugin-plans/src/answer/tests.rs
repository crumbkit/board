//! `answer` records what the pane mod records: the answer, with the question
//! put back unchanged in the same put, or a refusal that says why not.

use std::rc::Rc;
use std::time::{Duration, SystemTime};

use board_ops::{Limit, Plugins};
use board_plugin::Target;
use board_scenario::Item;
use serde_json::json;

use super::*;
use crate::Plans;
use crate::open::tests::{Answering, body, host, plans_board};

const QUESTION: &str = "/projects/a/questions/q";
const BLOB: &str = "4b825dc642cb6eb9a060e54bf8d69288fbe4904d";

/// 2026-11-04T22:50:00Z.
fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_mins(29_897_210)
}

/// A question with options `a` and `b`, recommending `a`, and `fields`
/// added or replacing its own.
fn question(fields: &Value) -> Value {
    let mut fields = fields.clone();
    let defaults = json!({
        "options": [{"id": "a", "text": "Go."}, {"id": "b", "text": "Stay."}],
        "recommendation": "a",
        "why": "It is simpler.",
    });
    for (name, value) in defaults.as_object().unwrap() {
        if fields.get(name).is_none() {
            fields[name] = value.clone();
        }
    }
    body("2026-10-01T09:00:00Z", &fields)
}

/// The rows `QUESTION_SQL` returns for the question at `QUESTION` with
/// `fields`, of `kind`, and an answer at each of `answers`.
fn rows(kind: &str, fields: &Value, answers: &[&str]) -> Vec<Vec<Value>> {
    let asked = vec![
        json!("question"),
        json!(QUESTION),
        json!(kind),
        json!(BLOB),
        question(fields),
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

/// A host answering with `rows` at `now`.
fn at_now(rows: Vec<Vec<Value>>) -> Answering {
    let mut host = host(rows);
    host.settings.now = now();
    host
}

#[test]
fn an_answer_is_one_put_of_the_new_answer_and_the_question_unchanged() {
    let host = at_now(rows("plans/question/v1", &json!({}), &[]));
    let input = json!({"path": QUESTION, "option": "b", "session": "local_0c", "written_by": "board-app/phone"});
    let output = answer(&host, &input).unwrap();
    assert_eq!(
        output,
        json!({
            "answer": "/projects/a/answers/q",
            "question": QUESTION,
            "choice": "b",
            "words": "Stay.",
            "answered": "2026-11-04T22:50:00Z",
        })
    );
    assert_eq!(host.asked.take(), [(question_sql(QUESTION), Limit::ALL)]);
    let unchanged: Value = serde_json::from_str(question(&json!({})).as_str().unwrap()).unwrap();
    assert_eq!(
        host.puts.take(),
        [vec![
            json!({"path": "/projects/a/answers/q", "expect": "none", "record": {
                "$type": "plans/answer/v1",
                "question": QUESTION,
                "choice": "b",
                "words": "Stay.",
                "answered": "2026-11-04T22:50:00Z",
                "by": {"session": "local_0c"},
                "written_by": "board-app/phone",
            }}),
            json!({"path": QUESTION, "expect": BLOB, "record": unchanged}),
        ]]
    );
}

#[test]
fn the_owners_words_are_kept_verbatim_and_by_and_written_by_are_left_out_when_not_given() {
    let host = at_now(rows("plans/question/v1", &json!({}), &[]));
    let words = "  A, but only after the release.  ";
    let output = answer(
        &host,
        &json!({"path": QUESTION, "option": "a", "words": words}),
    )
    .unwrap();
    assert_eq!(
        (&output["choice"], &output["words"]),
        (&json!("a"), &json!(words))
    );
    let puts = host.puts.take();
    let record = puts[0][0]["record"].as_object().unwrap();
    let fields: Vec<&str> = record.keys().map(String::as_str).collect();
    assert_eq!(fields, ["$type", "answered", "choice", "question", "words"]);
}

#[test]
fn a_question_quoted_in_sql_is_read_by_its_own_path() {
    let path = "/projects/o'brien/questions/q";
    assert_eq!(
        question_sql(path),
        format!(
            "SELECT 'question' AS kind, logical, type, blob, body, '/' || CASE WHEN q.path LIKE \
             '%.json' THEN substr(q.path, 1, length(q.path) - 5) ELSE q.path END FROM records q \
             WHERE logical = '/projects/o''brien/questions/q' \
             UNION ALL SELECT 'answer', a.logical, a.type, a.blob, NULL, NULL FROM records a, records q \
             WHERE q.logical = '/projects/o''brien/questions/q' AND a.type = 'plans/answer/v1' \
             AND {}",
            crate::question::NAMES
        )
    );
}

/// Each refusal beside its twin, which differs in one thing and is admitted:
/// the input, the question's type, its fields and the answers that name it.
struct Case {
    code: &'static str,
    refused: (Value, &'static str, Value, &'static [&'static str]),
    admitted: (Value, &'static str, Value, &'static [&'static str]),
    names: &'static str,
}

fn cases() -> Vec<Case> {
    let input = |path: &str, option: &str| json!({"path": path, "option": option});
    let question = "plans/question/v1";
    vec![
        Case {
            code: "plans::answer::not_a_question_path",
            refused: (input("/projects/a/notes/q", "a"), question, json!({}), &[]),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "/projects/a/notes/q",
        },
        Case {
            code: "plans::answer::not_a_question_path",
            refused: (
                input("/projects/a/questions/", "a"),
                question,
                json!({}),
                &[],
            ),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "/projects/a/questions/",
        },
        Case {
            code: "plans::answer::not_a_question_path",
            refused: (input("/questions/q", "a"), question, json!({}), &[]),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "/questions/q",
        },
        Case {
            code: "plans::answer::not_a_question_path",
            refused: (
                input("/projects//questions/q", "a"),
                question,
                json!({}),
                &[],
            ),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "/projects//questions/q",
        },
        Case {
            code: "plans::answer::no_question",
            refused: (input(QUESTION, "a"), "plans/decision/v1", json!({}), &[]),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "plans/decision/v1",
        },
        Case {
            code: "plans::answer::already_answered",
            refused: (
                input(QUESTION, "a"),
                question,
                json!({}),
                &["/projects/a/answers/elsewhere"],
            ),
            admitted: (input(QUESTION, "a"), question, json!({}), &[]),
            names: "/projects/a/answers/elsewhere",
        },
        Case {
            code: "plans::answer::not_open",
            refused: (
                input(QUESTION, "a"),
                question,
                json!({"status": "withdrawn"}),
                &[],
            ),
            admitted: (
                input(QUESTION, "a"),
                question,
                json!({"status": "open"}),
                &[],
            ),
            names: "withdrawn",
        },
        Case {
            code: "plans::answer::unknown_option",
            refused: (input(QUESTION, "c"), question, json!({}), &[]),
            admitted: (input(QUESTION, "b"), question, json!({}), &[]),
            names: "a, b",
        },
        Case {
            code: "plans::answer::words_needed",
            refused: (input(QUESTION, "none"), question, json!({}), &[]),
            admitted: (
                json!({"path": QUESTION, "option": "none", "words": "Neither."}),
                question,
                json!({}),
                &[],
            ),
            names: "none",
        },
        Case {
            code: "plans::answer::words_needed",
            refused: (input(QUESTION, "none"), question, json!({}), &[]),
            admitted: (
                input(QUESTION, "none"),
                question,
                json!({"options": [{"id": "none", "text": "Neither; drop it."}]}),
                &[],
            ),
            names: "none",
        },
    ]
}

#[test]
fn each_refusal_has_its_own_code_and_says_what_was_wrong_and_its_twin_is_recorded() {
    for Case {
        code,
        refused,
        admitted,
        names,
    } in cases()
    {
        let run = |(input, kind, fields, answers): &(Value, &str, Value, &[&str])| {
            let host = at_now(rows(kind, fields, answers));
            (answer(&host, input), host.puts.take().len())
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

/// `none` that chooses no option stores no `choice`, and its twin, an
/// option whose id is `none`, stores it. Both report `none` as the choice.
#[test]
fn none_that_chooses_no_option_stores_no_choice_and_an_option_named_none_stores_it() {
    for (options, stored) in [
        (json!({}), None),
        (
            json!({"options": [{"id": "none", "text": "Neither; drop it."}]}),
            Some(json!("none")),
        ),
    ] {
        let host = at_now(rows("plans/question/v1", &options, &[]));
        let input = json!({"path": QUESTION, "option": "none", "words": "Neither."});
        let output = answer(&host, &input).unwrap();
        assert_eq!(output["choice"], "none", "{options}");
        let puts = host.puts.take();
        let record = puts[0][0]["record"].as_object().unwrap();
        assert_eq!(record.get("choice").cloned(), stored, "{options}");
    }
}

#[test]
fn an_option_whose_id_is_none_is_chosen_with_its_text() {
    let fields = json!({"options": [{"id": "none", "text": "Neither; drop it."}]});
    let host = at_now(rows("plans/question/v1", &fields, &[]));
    let output = answer(&host, &json!({"path": QUESTION, "option": "none"})).unwrap();
    assert_eq!(
        (&output["choice"], &output["words"]),
        (&json!("none"), &json!("Neither; drop it."))
    );
}

#[test]
fn a_question_body_that_is_not_an_object_is_no_question_and_an_object_is_answered() {
    for (body, admitted) in [
        (json!("[\"Which way?\"]"), false),
        (Value::Null, false),
        (question(&json!({})), true),
    ] {
        let rows = vec![vec![
            json!("question"),
            json!(QUESTION),
            json!("plans/question/v1"),
            json!(BLOB),
            body.clone(),
        ]];
        let result = answer(&at_now(rows), &json!({"path": QUESTION, "option": "a"}));
        match result {
            Ok(_) => assert!(admitted, "{body}"),
            Err(error) => {
                assert!(!admitted, "{body}: {}", error.message);
                assert_eq!(error.code, "plans::answer::no_question");
                assert!(
                    error
                        .message
                        .contains("does not read as a plans/question/v1")
                );
            }
        }
    }
}

#[test]
fn no_row_at_the_path_is_no_question_and_a_row_there_is_answered() {
    let refused = answer(
        &at_now(Vec::new()),
        &json!({"path": QUESTION, "option": "a"}),
    );
    assert_eq!(refused.unwrap_err().code, "plans::answer::no_question");
    let admitted = answer(
        &at_now(rows("plans/question/v1", &json!({}), &[])),
        &json!({"path": QUESTION, "option": "a"}),
    );
    assert!(admitted.is_ok());
}

/// A board holding the question at `QUESTION`, and a host over it at `now`.
fn on_board(fields: &Value) -> (board_scenario::Scenario, Answering) {
    let (scenario, board) = plans_board(&[("projects/a/questions/q", question(fields))]);
    let mut host = at_now(Vec::new());
    host.board = Some(board);
    (scenario, host)
}

#[test]
fn the_board_holds_the_answer_and_the_question_unchanged_after_the_call() {
    let (scenario, host) = on_board(&json!({"note": "Nobody wrote down the deadline."}));
    answer(&host, &json!({"path": QUESTION, "option": "a"})).unwrap();
    let read = |path: &str| -> Value {
        let text = std::fs::read_to_string(scenario.workdir().join(path)).unwrap();
        serde_json::from_str(&text).unwrap()
    };
    assert_eq!(
        read("projects/a/answers/q.json"),
        json!({
            "$type": "plans/answer/v1",
            "question": QUESTION,
            "choice": "a",
            "words": "Go.",
            "answered": "2026-11-04T22:50:00Z",
        })
    );
    let expected: Value = serde_json::from_str(
        question(&json!({"note": "Nobody wrote down the deadline."}))
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(read("projects/a/questions/q.json"), expected);
    assert_eq!(
        expected["status"], "open",
        "an answer leaves the status open"
    );
}

#[test]
fn a_question_changed_after_the_read_is_refused_by_board_and_nothing_is_written() {
    for changed in [true, false] {
        let (scenario, host) = on_board(&json!({}));
        let scenario = Rc::new(scenario);
        if changed {
            let edited = question(&json!({"blocked_work": "the release"}));
            let there = Rc::clone(&scenario);
            *host.between.borrow_mut() = Some(Box::new(move || {
                there.write(
                    "projects/a/questions/q.json",
                    Item::File(edited.as_str().unwrap()),
                );
            }));
        }
        let result = answer(&host, &json!({"path": QUESTION, "option": "a"}));
        let answer_file = scenario.workdir().join("projects/a/answers/q.json");
        if changed {
            let refused = result.unwrap_err();
            assert_eq!(refused.code, "plans::board::refused");
            assert_eq!(refused.fields["code"], json!("board::put::stale"));
            assert!(!answer_file.exists(), "Board writes both lines or neither");
        } else {
            assert!(result.is_ok());
            assert!(answer_file.exists());
        }
    }
}

#[test]
fn board_calls_plans_answer_in_its_own_process_and_a_second_answer_is_refused() {
    let (scenario, board) = plans_board(&[("projects/a/questions/q", question(&json!({})))]);
    let target: Target = "plans.answer".parse().unwrap();
    let input = format!(r#"{{"path": "{QUESTION}", "option": "none", "words": "Neither."}}"#);
    let report = board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap();
    assert_eq!(report.output["answer"], "/projects/a/answers/q");
    assert_eq!(report.output["choice"], "none");
    assert_eq!(board.take_trace().work.get("started"), Some(&0));
    assert!(
        scenario
            .workdir()
            .join("projects/a/answers/q.json")
            .exists()
    );
    let board = board_ops::Board::open(scenario.workdir(), None).map(|mut board| {
        board.build_in(&[&Plans]);
        board
    });
    let refused = board
        .unwrap()
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap_err()
        .refusal();
    assert_eq!(refused.code.as_str(), "plans::answer::already_answered");
}

/// Each actor input reaches the answer's `by`, and a blank agent name
/// names nobody.
#[test]
fn every_actor_input_reaches_the_answers_by_and_a_blank_agent_is_left_out() {
    let cases = [
        (
            json!({"program": "codex", "person": "luke", "agent": "Inbox builder"}),
            json!({"program": "codex", "person": "luke", "agent": "Inbox builder"}),
        ),
        (
            json!({"person": "luke", "agent": " "}),
            json!({"person": "luke"}),
        ),
    ];
    for (given, expected) in cases {
        let host = at_now(rows("plans/question/v1", &json!({}), &[]));
        let mut input = json!({"path": QUESTION, "option": "b"});
        input
            .as_object_mut()
            .unwrap()
            .extend(given.as_object().unwrap().clone());
        answer(&host, &input).unwrap();
        let puts = host.puts.take();
        assert_eq!(puts[0][0]["record"]["by"], expected, "{given}");
    }
}

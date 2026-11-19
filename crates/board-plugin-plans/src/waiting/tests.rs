//! `waiting` counts what `open` lists and says it in one line.

use serde_json::{Value, json};

use super::*;
use board_ops::Plugins;
use board_plugin::Target;

use crate::open::tests::{body, host, plans_board};

/// A row of an open question at `path` that blocks no work.
fn row(path: &str) -> Vec<Value> {
    vec![
        json!(path),
        Value::Null,
        body("2026-10-01T09:00:00Z", &json!({})),
        Value::Null,
    ]
}

#[test]
fn waiting_says_none_one_or_many_questions_are_waiting() {
    for (rows, count, line) in [
        (vec![], 0, "No questions are waiting for you."),
        (
            vec![row("/projects/a/questions/q")],
            1,
            "1 question is waiting for you. Run /ask to answer it.",
        ),
        (
            vec![
                row("/projects/a/questions/q"),
                row("/projects/b/questions/r"),
            ],
            2,
            "2 questions are waiting for you. Run /ask to answer them.",
        ),
    ] {
        let output = waiting(&host(rows), &json!({})).unwrap();
        assert_eq!(
            output,
            json!({"total": count, "blocking": 0, "unreadable": 0, "line": line})
        );
    }
}

/// A path counts only the questions below it, as `open` lists them, and an
/// unreadable question is counted apart and named in the line.
#[test]
fn waiting_counts_below_a_path_and_names_unreadable_questions_apart() {
    let rows = vec![
        row("/projects/a/questions/q"),
        row("/projects/b/questions/r"),
        vec![
            json!("/projects/a/questions/bad"),
            Value::Null,
            Value::Null,
            Value::Null,
        ],
    ];
    let output = waiting(&host(rows), &json!({"path": "/projects/a"})).unwrap();
    assert_eq!(
        output,
        json!({
            "total": 1,
            "blocking": 0,
            "unreadable": 1,
            "line": "1 question is waiting for you, and 1 more cannot be read. Run /ask to answer it."
        })
    );
}

/// Board refuses an input field `waiting` does not take with its input code,
/// and admits the twin without the field, counting the board's questions.
#[test]
fn board_refuses_an_unknown_input_field_and_admits_its_twin() {
    let (_scenario, board) = plans_board(&[(
        "projects/a/questions/q",
        body("2026-10-01T09:00:00Z", &json!({})),
    )]);
    let target: Target = "plans.waiting".parse().unwrap();
    let refused = board
        .call(&target, br#"{"limit": 3}"#, &Plugins::default())
        .unwrap_err();
    assert_eq!(refused.refusal().code.as_str(), "board::plugin::bad_input");
    let report = board.call(&target, b"{}", &Plugins::default()).unwrap();
    assert_eq!(report.output["total"], json!(1));
}

/// `blocking` counts the questions that block active work, and the line
/// says so; a question whose work is done does not count.
#[test]
fn waiting_counts_the_questions_that_block_active_work() {
    let blocking = |path: &str, work: &str| {
        vec![
            json!(path),
            Value::Null,
            body("2026-10-01T09:00:00Z", &json!({})),
            json!(work),
        ]
    };
    let rows = vec![
        blocking("/projects/a/questions/q", "active"),
        blocking("/projects/a/questions/r", "active"),
        blocking("/projects/a/questions/s", "done"),
    ];
    let output = waiting(&host(rows), &json!({})).unwrap();
    assert_eq!(
        (&output["total"], &output["blocking"], &output["line"]),
        (
            &json!(3),
            &json!(2),
            &json!(
                "3 questions are waiting for you, 2 of them blocking work. Run /ask to answer them."
            )
        )
    );
    let rows = vec![blocking("/projects/a/questions/q", "active")];
    let output = waiting(&host(rows), &json!({})).unwrap();
    assert_eq!(
        output["line"],
        json!("1 question is waiting for you, and it blocks work. Run /ask to answer it.")
    );
}

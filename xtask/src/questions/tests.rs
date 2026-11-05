//! The migration reopens each question stored as answered, changes nothing
//! else, and reports each question whose status and answers disagreed.

use std::fs;

use serde_json::json;

use super::*;
use crate::tests::Fixture;

/// A board in a temporary directory holding `files`, by path from its root.
fn board(files: &[(&str, String)]) -> Fixture {
    let board = Fixture::new();
    for (path, text) in files {
        board.write(path, text);
    }
    board
}

/// A question stored with `status`, as Board writes a record.
fn question(status: &str) -> String {
    let record = json!({
        "$type": "plans/question/v1",
        "asked": "2026-10-01T09:00:00Z",
        "options": [{"id": "a", "text": "Say \"status\": \"answered\" here."}],
        "question": "Which way?",
        "session": "local_0a1b",
        "status": status,
    });
    format!("{}\n", serde_json::to_string_pretty(&record).unwrap())
}

fn answer(question: &str) -> String {
    let record = json!({
        "$type": "plans/answer/v1",
        "answered": "2026-10-02T09:00:00Z",
        "choice": "a",
        "question": question,
        "words": "A.",
    });
    format!("{}\n", serde_json::to_string_pretty(&record).unwrap())
}

fn read(board: &Fixture, path: &str) -> String {
    fs::read_to_string(board.path().join(path)).unwrap()
}

/// Four questions, one of each pairing of stored status and answer, plus a
/// withdrawn one, an answer naming no question and a hidden directory.
fn every_case() -> Fixture {
    board(&[
        ("projects/a/questions/agrees.json", question("answered")),
        (
            "projects/a/answers/agrees.json",
            answer("/projects/a/questions/agrees"),
        ),
        ("projects/a/questions/unanswered.json", question("answered")),
        ("projects/a/questions/answered.json", question("open")),
        (
            "projects/a/answers/later.json",
            answer("/projects/a/questions/answered"),
        ),
        ("projects/a/questions/waiting.json", question("open")),
        ("projects/a/questions/gone.json", question("withdrawn")),
        ("projects/a/questions/closed.json", question("closed")),
        (
            "projects/a/answers/lost.json",
            answer("/projects/a/questions/nowhere"),
        ),
        (".board/types/plans/question/v1.json", question("answered")),
        ("projects/a/notes.json", "not json".to_owned()),
    ])
}

#[test]
fn each_answered_question_becomes_open_and_each_disagreement_is_reported() {
    let board = every_case();
    let migrated = migrate(board.path(), false).unwrap();
    assert_eq!(
        migrated,
        Migrated {
            questions: [
                ("answered", 2),
                ("closed", 1),
                ("open", 2),
                ("withdrawn", 1)
            ]
            .map(|(status, count)| (status.to_owned(), count))
            .into(),
            answers: 3,
            rewritten: vec![
                "/projects/a/questions/agrees".to_owned(),
                "/projects/a/questions/unanswered".to_owned(),
            ],
            disagreed: vec![
                Disagreement {
                    question: "/projects/a/questions/answered".to_owned(),
                    status: "open".to_owned(),
                    answers: vec!["/projects/a/answers/later".to_owned()],
                },
                Disagreement {
                    question: "/projects/a/questions/closed".to_owned(),
                    status: "closed".to_owned(),
                    answers: Vec::new(),
                },
                Disagreement {
                    question: "/projects/a/questions/unanswered".to_owned(),
                    status: "answered".to_owned(),
                    answers: Vec::new(),
                },
            ],
            dangling: vec!["/projects/a/answers/lost".to_owned()],
            by_hand: Vec::new(),
        }
    );
    assert_eq!(
        read(&board, "projects/a/questions/agrees.json"),
        question("open")
    );
    assert_eq!(
        read(&board, "projects/a/questions/unanswered.json"),
        question("open")
    );
    assert_eq!(
        read(&board, "projects/a/questions/gone.json"),
        question("withdrawn")
    );
    assert_eq!(
        read(&board, ".board/types/plans/question/v1.json"),
        question("answered"),
        "a directory whose name starts with a dot is not read"
    );
}

#[test]
fn a_dry_run_reports_the_same_and_writes_nothing() {
    let board = every_case();
    let dry = migrate(board.path(), true).unwrap();
    assert_eq!(
        read(&board, "projects/a/questions/agrees.json"),
        question("answered")
    );
    let written = migrate(board.path(), false).unwrap();
    assert_eq!(dry, written);
    let again = migrate(board.path(), false).unwrap();
    assert_eq!(
        again.rewritten,
        Vec::<String>::new(),
        "a second run has nothing to do"
    );
}

#[test]
fn only_the_status_bytes_change_and_a_status_spelled_twice_is_left_for_a_person() {
    let compact = r#"{"$type":"plans/question/v1","status" :  "answered","question":"Q"}"#;
    assert_eq!(
        reopened(compact).as_deref(),
        Some(r#"{"$type":"plans/question/v1","status" :  "open","question":"Q"}"#)
    );
    let twice = r#"{"$type":"plans/question/v1","status":"answered","note":{"status":"answered"}}"#;
    assert_eq!(reopened(twice), None);
    let board = board(&[("projects/a/questions/q.json", twice.to_owned())]);
    let migrated = migrate(board.path(), false).unwrap();
    assert_eq!(migrated.by_hand, ["/projects/a/questions/q"]);
    assert_eq!(read(&board, "projects/a/questions/q.json"), twice);
}

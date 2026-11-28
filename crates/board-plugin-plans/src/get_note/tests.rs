use board_model::{Code, LogicalPath};
use board_ops::{Asked, Beside, Plugin};
use serde_json::{Value, json};

use crate::Plans;

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// A question read at `asked`, held at `held` in the board that holds it,
/// in `status`.
fn question(asked: &str, held: &str, status: &str) -> Asked {
    let Value::Object(record) = json!({
        "$type": "plans/question/v1",
        "question": "Ship it?",
        "by": {"session": "local_0a1b"},
        "asked": "2026-10-01T09:00:00Z",
        "status": status,
    }) else {
        unreachable!()
    };
    Asked {
        path: path(asked),
        display: path(asked),
        held: held.to_owned(),
        record,
    }
}

/// An answer to the question at `question`, choosing `choice`, in `words`.
fn answer(question: &str, choice: Option<&str>, words: &str) -> Value {
    let mut answer = json!({
        "$type": "plans/answer/v1",
        "question": question,
        "words": words,
        "answered": "2026-10-02T09:00:00Z",
    });
    if let Some(choice) = choice {
        answer["choice"] = json!(choice);
    }
    answer
}

/// The one note plans declares, for questions.
fn note() -> &'static board_ops::GetNote {
    let [note] = Plans.get_notes() else {
        panic!("plans declares one note")
    };
    assert_eq!(note.record_type, "plans/question/v1");
    note
}

#[test]
fn the_path_beside_an_open_question_is_its_same_id_answer_under_the_same_mount() {
    let beside = |asked: &Asked| (note().beside)(asked).map(|path| path.to_string());
    assert_eq!(
        beside(&question(
            "/projects/a/questions/q",
            "/projects/a/questions/q",
            "open"
        )),
        Some("/projects/a/answers/q".to_owned())
    );
    assert_eq!(
        beside(&question(
            "/repos/notes/projects/a/questions/q",
            "/projects/a/questions/q",
            "open"
        )),
        Some("/repos/notes/projects/a/answers/q".to_owned())
    );
}

#[test]
fn a_question_in_another_status_reads_nothing_beside_it() {
    let withdrawn = question(
        "/projects/a/questions/q",
        "/projects/a/questions/q",
        "withdrawn",
    );
    assert_eq!((note().beside)(&withdrawn), None);
    let mut no_status = question("/projects/a/questions/q", "/projects/a/questions/q", "open");
    no_status.record.remove("status");
    assert_eq!((note().beside)(&no_status), None);
}

#[test]
fn an_answer_that_names_the_question_by_either_path_is_named_with_its_choice_and_words() {
    let at = path("/repos/notes/projects/a/answers/q");
    let asked = question(
        "/repos/notes/projects/a/questions/q",
        "/projects/a/questions/q",
        "open",
    );
    let line = |record: &Value| (note().line)(&asked, &Beside::Record { path: &at, record });
    let named = "/repos/notes/projects/a/questions/q: answered by /repos/notes/projects/a/answers/q (2026-10-02T09:00:00Z): ship, \"Yes, ship it.\"";
    for question in [
        "/repos/notes/projects/a/questions/q",
        "/projects/a/questions/q",
    ] {
        let record = answer(question, Some("ship"), "Yes, ship it.");
        assert_eq!(line(&record).as_deref(), Some(named), "{question}");
    }
    let long = "a".repeat(79) + "bcd\nefg";
    let unchosen = line(&answer("/projects/a/questions/q", None, &long)).unwrap();
    assert!(
        unchosen.ends_with(&format!("): none, \"{}b\"", "a".repeat(79))),
        "the first 80 characters of the words, and none for no choice: {unchosen}"
    );
    let lines = line(&answer("/projects/a/questions/q", None, "one\ntwo")).unwrap();
    assert!(lines.ends_with("\"one two\""), "{lines}");
}

#[test]
fn nothing_there_or_an_answer_to_another_question_is_no_answer_and_names_refs() {
    let at = path("/projects/a/answers/q");
    let asked = question("/projects/a/questions/q", "/projects/a/questions/q", "open");
    let none = "/projects/a/questions/q: no answer at /projects/a/answers/q; run board refs /projects/a/questions/q to find an answer at another path";
    let other = answer("/projects/a/questions/other", Some("ship"), "Yes.");
    let task = json!({"$type": "lr/task/v1", "question": "/projects/a/questions/q"});
    for beside in [
        Beside::Nothing { path: &at },
        Beside::Record {
            path: &at,
            record: &other,
        },
        Beside::Record {
            path: &at,
            record: &task,
        },
    ] {
        assert_eq!(
            (note().line)(&asked, &beside).as_deref(),
            Some(none),
            "{beside:?}"
        );
    }
}

#[test]
fn a_failed_read_beside_a_question_names_its_code() {
    let at = path("/projects/a/answers/q");
    let asked = question("/projects/a/questions/q", "/projects/a/questions/q", "open");
    let failed = Beside::Failed {
        path: &at,
        code: Code::RepoUnreadable,
    };
    assert_eq!(
        (note().line)(&asked, &failed),
        Some(format!(
            "/projects/a/questions/q: answer not read: {}",
            Code::RepoUnreadable
        ))
    );
}

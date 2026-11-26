//! A command-line put fills the fields a type declares and drops the
//! fields its schema does not name, before the check.

use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};

use board_model::{Code, Lined};
use board_scenario::{Item, Scenario};
use serde_json::{Map, Value, json};

use super::{board, file, path};
use crate::put::{Dropped, Expect, Fill, Mending, Mends, PutError, PutReport, With};
use crate::{Board, Settings, Variables};

/// A question: who asked, when, its status and options, each closed.
const QUESTION_SCHEMA: &str = r#"{
    "type": "object",
    "x-board-folder": "/q/{project}",
    "additionalProperties": false,
    "required": ["$type", "question", "by", "asked", "status"],
    "properties": {
        "$type": {"const": "lr/question/v1"},
        "question": {"type": "string"},
        "by": {
            "type": "object",
            "additionalProperties": false,
            "properties": {"session": {"type": "string", "pattern": "^local_[0-9a-f-]+$"}}
        },
        "asked": {"type": "string", "format": "date-time"},
        "status": {"enum": ["open", "withdrawn"]},
        "options": {
            "type": "array",
            "items": {"type": "object", "additionalProperties": false, "properties": {"id": {"type": "string"}}}
        }
    }
}"#;

/// A decision with its alternatives, closed, and a `why` that must be text.
const DECISION_SCHEMA: &str = r#"{
    "type": "object",
    "additionalProperties": false,
    "required": ["$type", "decision"],
    "properties": {
        "$type": {"const": "lr/decision/v1"},
        "decision": {"type": "string"},
        "why": {"type": "string"},
        "alternatives": {"type": "array", "items": {"type": "string"}}
    }
}"#;

const SESSION: &str = "local_84ccdcef-4c52-487a-9ac6-ebf2f157e440";

/// 2026-11-25T01:04:06Z.
const NOW: u64 = 1_795_568_646;

/// Fails as a fill that cannot reach its commits does.
fn unreachable(_: &Map<String, Value>, _: &Settings) -> Result<Option<Value>, String> {
    Err("abc names no commit in /checkout".to_owned())
}

static FILLS: [Fill; 4] = [
    Fill {
        record_type: "lr/question/v1",
        field: "by",
        with: With::Session,
    },
    Fill {
        record_type: "lr/question/v1",
        field: "asked",
        with: With::Now,
    },
    Fill {
        record_type: "lr/question/v1",
        field: "status",
        with: With::Text("open"),
    },
    Fill {
        record_type: "lr/decision/v1",
        field: "why",
        with: With::Derived(unreachable),
    },
];

/// The test board, with the question and decision types.
fn questions() -> Scenario {
    let scenario = board();
    scenario
        .write(
            ".board/types/lr/question/v1.schema.json",
            Item::File(QUESTION_SCHEMA),
        )
        .write(
            ".board/types/lr/decision/v1.schema.json",
            Item::File(DECISION_SCHEMA),
        );
    scenario
}

fn settings() -> Settings {
    Settings {
        now: UNIX_EPOCH + Duration::from_secs(NOW),
        day_start: None,
        utc_offset: 0,
        caller: None,
        environment: Variables::default(),
        cache_home: None,
    }
}

/// The board in `scenario`, mending its puts with `session`.
fn mended(scenario: &Scenario, session: Option<&str>) -> Board {
    let mut board = crate::tests::open(scenario);
    let mending = Mending {
        session: session.map(str::to_owned),
        settings: settings(),
    };
    board.mend_with(mending, FILLS.iter().collect());
    board
}

fn put(board: &Board, at: &str, record: &Value) -> Result<PutReport, PutError> {
    board.put(&path(at), Expect::Any, record.to_string().as_bytes())
}

/// The record written at `physical`.
fn written(scenario: &Scenario, physical: &str) -> Value {
    serde_json::from_slice(&file(scenario, physical).unwrap()).unwrap()
}

fn mends(filled: &[&str], dropped: &[(&str, Option<&str>)], unfilled: &[&str]) -> Mends {
    Mends {
        line: None,
        unfilled: unfilled.iter().map(|why| (*why).to_owned()).collect(),
        filled: filled.iter().map(|pointer| (*pointer).to_owned()).collect(),
        dropped: (dropped.iter())
            .map(|(pointer, near)| Dropped {
                pointer: (*pointer).to_owned(),
                near: near.map(str::to_owned),
            })
            .collect(),
    }
}

#[test]
fn a_question_with_no_by_asked_or_status_is_written_with_each_filled() {
    let scenario = questions();
    let question = json!({"$type": "lr/question/v1", "question": "Which?"});
    let report = put(&mended(&scenario, Some(SESSION)), "/q/a", &question).unwrap();
    assert_eq!(
        written(&scenario, "q/a.json"),
        json!({
            "$type": "lr/question/v1",
            "question": "Which?",
            "by": {"session": SESSION},
            "asked": "2026-11-25T01:04:06Z",
            "status": "open",
        })
    );
    assert_eq!(report.mended.len(), 1);
    assert_eq!(report.mended[0].path.to_string(), "/q/a");
    assert_eq!(
        report.mended[0].mends,
        mends(&["/by", "/asked", "/status"], &[], &[])
    );
}

#[test]
fn a_field_the_caller_gave_is_kept_and_not_filled() {
    let scenario = questions();
    let question = json!({
        "$type": "lr/question/v1",
        "question": "Which?",
        "by": {"session": "local_1"},
        "asked": "2026-01-01T00:00:00Z",
        "status": "withdrawn",
    });
    let report = put(&mended(&scenario, Some(SESSION)), "/q/a", &question).unwrap();
    assert_eq!(written(&scenario, "q/a.json"), question);
    assert!(report.mended.is_empty(), "{:?}", report.mended);
}

#[test]
fn with_no_session_a_question_with_no_by_is_refused_as_before() {
    let scenario = questions();
    let question = json!({"$type": "lr/question/v1", "question": "Which?"});
    let error = put(&mended(&scenario, None), "/q/a", &question).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    let message = error.to_string();
    assert!(message.contains("\"by\""), "{message}");
    assert!(
        message.contains("could not fill /by: the put has no session; set BOARD_SESSION"),
        "{message}"
    );
    assert_eq!(file(&scenario, "q/a.json"), None);
}

#[test]
fn a_put_without_mending_fills_nothing_and_refuses_a_question_with_no_status() {
    let scenario = questions();
    let question = json!({
        "$type": "lr/question/v1",
        "question": "Which?",
        "by": {"session": SESSION},
        "asked": "2026-01-01T00:00:00Z",
    });
    let error = put(&crate::tests::open(&scenario), "/q/a", &question).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    let mut whole = question;
    whole["status"] = json!("open");
    assert!(put(&crate::tests::open(&scenario), "/q/a", &whole).is_ok());
}

#[test]
fn a_decision_with_an_extra_summary_is_written_without_it() {
    let scenario = questions();
    let decision = json!({"$type": "lr/decision/v1", "decision": "d", "why": "w", "summary": "s"});
    let report = put(&mended(&scenario, Some(SESSION)), "/d/a", &decision).unwrap();
    assert_eq!(
        written(&scenario, "d/a.json"),
        json!({"$type": "lr/decision/v1", "decision": "d", "why": "w"})
    );
    assert_eq!(
        report.mended[0].mends,
        mends(&[], &[("/summary", None)], &[])
    );
}

#[test]
fn without_mending_a_decision_with_an_extra_summary_is_refused_at_its_root() {
    let scenario = questions();
    let decision = json!({"$type": "lr/decision/v1", "decision": "d", "why": "w", "summary": "s"});
    let error = put(&crate::tests::open(&scenario), "/d/a", &decision).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    assert_eq!(error.fields().pointer.as_deref(), Some(""));
    assert_eq!(file(&scenario, "d/a.json"), None);
}

#[test]
fn a_misspelled_field_of_a_field_the_record_lacks_is_kept_and_refused_naming_that_field() {
    let scenario = questions();
    let decision =
        json!({"$type": "lr/decision/v1", "decision": "d", "why": "w", "alternative": ["x"]});
    let error = put(&mended(&scenario, Some(SESSION)), "/d/a", &decision).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    let message = error.to_string();
    assert!(
        message.contains(
            "board put kept /alternative, which its type does not name, since it is one edit from /alternatives, which the record lacks; rename it"
        ),
        "{message}"
    );
    assert_eq!(file(&scenario, "d/a.json"), None);
}

#[test]
fn a_misspelled_field_of_a_field_the_record_holds_is_dropped_naming_that_field() {
    let scenario = questions();
    let decision = json!({
        "$type": "lr/decision/v1",
        "decision": "d",
        "why": "w",
        "alternatives": ["y"],
        "alternative": ["x"],
    });
    let report = put(&mended(&scenario, Some(SESSION)), "/d/a", &decision).unwrap();
    assert_eq!(
        report.mended[0].mends,
        mends(&[], &[("/alternative", Some("/alternatives"))], &[])
    );
    assert_eq!(written(&scenario, "d/a.json")["alternatives"], json!(["y"]));
}

#[test]
fn a_nested_unknown_field_is_dropped_by_its_pointer() {
    let scenario = questions();
    let question = json!({
        "$type": "lr/question/v1",
        "question": "Which?",
        "options": [{"id": "a", "label": "A"}],
    });
    let report = put(&mended(&scenario, Some(SESSION)), "/q/a", &question).unwrap();
    assert_eq!(
        written(&scenario, "q/a.json")["options"],
        json!([{"id": "a"}])
    );
    assert_eq!(
        report.mended[0].mends.dropped[0].pointer,
        "/options/0/label"
    );
}

#[test]
fn a_record_still_refused_after_a_drop_names_the_fields_dropped() {
    let scenario = questions();
    let decision = json!({"$type": "lr/decision/v1", "decision": 5, "why": "w", "summary": "s"});
    let error = put(&mended(&scenario, Some(SESSION)), "/d/a", &decision).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    assert_eq!(error.fields().pointer.as_deref(), Some("/decision"));
    let message = error.to_string();
    assert!(
        message.contains("dropped /summary, which its type does not name"),
        "{message}"
    );
}

#[test]
fn a_fill_that_cannot_be_made_leaves_the_field_out_and_the_refusal_says_why() {
    let scenario = questions();
    let decision = json!({"$type": "lr/decision/v1", "decision": 5});
    let error = put(&mended(&scenario, Some(SESSION)), "/d/a", &decision).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    let message = error.to_string();
    assert!(
        message.contains("could not fill /why: abc names no commit in /checkout"),
        "{message}"
    );
    let admitted = json!({"$type": "lr/decision/v1", "decision": "d"});
    let report = put(&mended(&scenario, Some(SESSION)), "/d/a", &admitted).unwrap();
    assert_eq!(
        report.mended[0].mends,
        mends(&[], &[], &["/why: abc names no commit in /checkout"])
    );
}

#[test]
fn a_new_put_in_its_types_folder_fills_and_drops_as_a_put_at_a_path_does() {
    let scenario = questions();
    let question = json!({"$type": "lr/question/v1", "question": "Which?", "summary": "s"});
    let vars = [("project".to_owned(), "p".to_owned())]
        .into_iter()
        .collect();
    let report = mended(&scenario, Some(SESSION))
        .put_declared_waiting(
            &vars,
            question.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap();
    let display = report.writes[0].display.to_string();
    assert!(display.starts_with("/q/p/"), "{display}");
    assert_eq!(
        report.mended[0].mends,
        mends(&["/by", "/asked", "/status"], &[("/summary", None)], &[])
    );
    let error = crate::tests::open(&scenario)
        .put_declared_waiting(
            &vars,
            question.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
}

#[test]
fn a_new_put_in_a_named_folder_fills_and_drops_as_a_put_at_a_path_does() {
    let scenario = questions();
    let question = json!({"$type": "lr/question/v1", "question": "Which?", "summary": "s"});
    let report = mended(&scenario, Some(SESSION))
        .put_new_waiting(
            &path("/q"),
            question.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        report.mended[0].mends,
        mends(&["/by", "/asked", "/status"], &[("/summary", None)], &[])
    );
    let error = crate::tests::open(&scenario)
        .put_new_waiting(
            &path("/q"),
            question.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
}

#[test]
fn a_batch_reports_each_fill_and_drop_under_its_line() {
    let scenario = questions();
    let question = json!({"$type": "lr/question/v1", "question": "Which?"});
    let decision = json!({"$type": "lr/decision/v1", "decision": "d", "why": "w", "notes": "n"});
    let input = [
        json!({"path": "/d/plain", "record": {"$type": "lr/decision/v1", "decision": "d", "why": "w"}}),
        json!({"path": "/q/a", "record": question}),
        json!({"new": "/d/", "record": decision}),
    ]
    .map(|line| line.to_string() + "\n")
    .concat();
    let report = mended(&scenario, Some(SESSION))
        .put_batch(&mut input.as_bytes())
        .unwrap();
    let lines: Vec<_> = (report.mended.iter())
        .map(|mended| {
            (
                mended.mends.line,
                mended.mends.filled.len(),
                mended.mends.dropped.len(),
            )
        })
        .collect();
    assert_eq!(lines, [(Some(2), 3, 0), (Some(3), 0, 1)]);
    assert_eq!(report.mended[1].path, report.writes[2].display);
}

#[test]
fn a_batch_with_a_line_still_refused_writes_nothing() {
    let scenario = questions();
    let input = [
        json!({"path": "/q/a", "record": {"$type": "lr/question/v1", "question": "Which?"}}),
        json!({"path": "/d/a", "record": {"$type": "lr/decision/v1", "decision": 5, "x": 1}}),
    ]
    .map(|line| line.to_string() + "\n")
    .concat();
    let error = mended(&scenario, Some(SESSION))
        .put_batch(&mut input.as_bytes())
        .unwrap_err();
    let PutError::Batch { refusals, .. } = error else {
        panic!("not a batch refusal: {error}");
    };
    let refused: Vec<_> = (refusals.iter())
        .map(|Lined { line, code, .. }| (*line, *code))
        .collect();
    assert_eq!(refused, [(2, Code::RecordInvalid)]);
    assert!(
        refusals[0].message.contains("dropped /x"),
        "{}",
        refusals[0].message
    );
    assert_eq!(file(&scenario, "q/a.json"), None);
}

#[test]
fn a_session_comes_from_board_session_or_else_a_local_host_session() {
    let os = |text: &'static str| Some(OsStr::new(text));
    let cases = [
        (os(SESSION), os("local_9"), Some(SESSION)),
        (None, os("local_9ba41ad4-5503"), Some("local_9ba41ad4-5503")),
        (None, os("cse_9ba41ad4"), None),
        (os("Local_9"), None, None),
        (os("local_"), None, None),
        (os("local_9G"), os("local_9"), Some("local_9")),
        (os("bogus"), os("local_ab"), Some("local_ab")),
        (None, None, None),
    ];
    for (board, host, expected) in cases {
        assert_eq!(
            Mending::session(board, host).as_deref(),
            expected,
            "{board:?} {host:?}"
        );
    }
}

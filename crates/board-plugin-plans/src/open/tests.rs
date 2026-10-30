//! `open` answers as the Python plugin's `open.py` did: the same readable
//! rule, the same order and the same counts.

use std::cell::RefCell;

use board_ops::{Board, Plugin, Plugins, QueryReport, Settings};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::*;
use crate::Plans;

/// A host that answers every query with `rows` and records what it was
/// asked, or answers with `failing`'s refusal of a query that names no
/// table.
struct Answering {
    rows: Vec<Vec<Value>>,
    asked: RefCell<Vec<(String, Limit)>>,
    failing: Option<Board>,
    settings: Settings,
}

impl Host for Answering {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, board_ops::QueryError> {
        self.asked.borrow_mut().push((sql.to_owned(), limit));
        if let Some(board) = &self.failing {
            board.query("SELECT logical FROM nowhere", 1, None, None)?;
        }
        Ok(QueryReport {
            columns: vec!["logical".into(), "code".into(), "body".into()],
            rows: self.rows.clone(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

fn host(rows: Vec<Vec<Value>>) -> Answering {
    Answering {
        rows,
        asked: RefCell::default(),
        failing: None,
        settings: Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            caller: None,
        },
    }
}

/// A `plans/question/v1` body asked at `asked`, with `fields` added or
/// replacing its own.
fn body(asked: &str, fields: &Value) -> Value {
    let mut record = json!({
        "$type": "plans/question/v1",
        "question": "Which way?",
        "session": "local_0a1b",
        "asked": asked,
        "status": "open",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    Value::String(record.to_string())
}

/// A row of the question at `path` with `code` and `body`.
fn row(path: &str, code: Option<&str>, body: Value) -> Vec<Value> {
    vec![json!(path), json!(code), body]
}

/// The paths `open` returns from `rows` for `input`, and its output.
fn opened(rows: Vec<Vec<Value>>, input: &Value) -> (Vec<String>, Value) {
    let output = open(&host(rows), input).unwrap();
    let paths = output["questions"].as_array().unwrap().iter();
    let paths = paths.map(|question| question["path"].as_str().unwrap().to_owned());
    (paths.collect(), output)
}

#[test]
fn open_asks_for_every_open_question_once_with_no_row_limit() {
    let host = host(Vec::new());
    open(&host, &json!({})).unwrap();
    assert_eq!(host.asked.take(), [(QUESTIONS.to_owned(), Limit::ALL)]);
}

#[test]
fn questions_come_oldest_first_and_the_same_instant_in_other_offsets_is_ordered_by_path() {
    let rows = vec![
        row(
            "/p/questions/d",
            None,
            body("2026-10-01T10:00:00Z", &json!({})),
        ),
        row(
            "/p/questions/c",
            None,
            body("2026-10-01T11:00:00+02:00", &json!({})),
        ),
        row(
            "/p/questions/b",
            None,
            body("2026-10-01T09:00:00Z", &json!({})),
        ),
        row(
            "/p/questions/a",
            None,
            body("2026-10-01T09:00:00.0000001Z", &json!({})),
        ),
    ];
    let (paths, _) = opened(rows, &json!({}));
    assert_eq!(
        paths,
        [
            "/p/questions/a",
            "/p/questions/b",
            "/p/questions/c",
            "/p/questions/d"
        ]
    );
}

#[test]
fn a_path_matches_the_questions_below_it_with_or_without_a_trailing_slash() {
    let rows = || {
        [
            "/projects/a/questions/q",
            "/projects/ab/questions/q",
            "/projects/a",
        ]
        .map(|path| row(path, None, body("2026-10-01T09:00:00Z", &json!({}))))
        .to_vec()
    };
    for (path, expected) in [
        ("/projects/a", vec!["/projects/a/questions/q"]),
        ("/projects/a/", vec!["/projects/a/questions/q"]),
        ("/projects/a//", vec!["/projects/a/questions/q"]),
        (
            "/",
            vec![
                "/projects/a",
                "/projects/a/questions/q",
                "/projects/ab/questions/q",
            ],
        ),
    ] {
        let (paths, output) = opened(rows(), &json!({"path": path}));
        assert_eq!(paths, expected, "{path}");
        assert_eq!(output["unreadable"], 0, "{path}");
    }
    let (paths, _) = opened(rows(), &json!({}));
    assert_eq!(paths.len(), 3, "no path is every question");
}

#[test]
fn the_limit_cuts_the_questions_but_not_the_total_and_says_it_cut() {
    let rows = || {
        (0..5)
            .map(|at| {
                row(
                    &format!("/q/{at}"),
                    None,
                    body(&format!("2026-10-0{}T09:00:00Z", at + 1), &json!({})),
                )
            })
            .collect::<Vec<_>>()
    };
    for (limit, shown, truncated) in [
        (json!(2), 2, true),
        (json!(5), 5, false),
        (json!(3.0), 3, true),
    ] {
        let (paths, output) = opened(rows(), &json!({"limit": limit}));
        assert_eq!(paths.len(), shown, "{limit}");
        assert_eq!(
            (&output["total"], &output["truncated"]),
            (&json!(5), &json!(truncated))
        );
    }
    let (paths, output) = opened(rows(), &json!({}));
    assert_eq!((paths.len(), &output["truncated"]), (5, &json!(false)));
}

#[test]
fn a_question_degraded_by_a_reference_is_read_and_one_degraded_by_its_own_content_is_unreadable() {
    let asked = || body("2026-10-01T09:00:00Z", &json!({}));
    let rows = vec![
        row("/q/dangling", Some("board::ref::dangling"), asked()),
        row("/q/wrong", Some("board::ref::wrong_type"), asked()),
        row("/q/invalid", Some("board::record::invalid"), asked()),
        row("/q/clean", None, asked()),
    ];
    let (paths, output) = opened(rows, &json!({}));
    assert_eq!(paths, ["/q/clean", "/q/dangling", "/q/wrong"]);
    assert_eq!(
        (&output["total"], &output["unreadable"]),
        (&json!(3), &json!(1))
    );
}

/// Each change that leaves a question unreadable, beside a twin that keeps
/// it readable.
fn twins() -> Vec<(Value, Value)> {
    let options = |options: Value| json!({"options": options});
    vec![
        (
            json!({"asked": "2026-10-01T09:00:00"}),
            json!({"asked": "2026-10-01T09:00:00Z"}),
        ),
        (
            json!({"asked": "2026-10-01"}),
            json!({"asked": "2026-10-01T00:00:00z"}),
        ),
        (
            json!({"asked": "2026-10-01T09:00:00+0000"}),
            json!({"asked": "2026-10-01T09:00:00+00:00"}),
        ),
        (
            json!({"asked": "2026-10-01T23:59:60Z"}),
            json!({"asked": "2026-10-01T23:59:59Z"}),
        ),
        (
            json!({"asked": "2026-02-30T09:00:00Z"}),
            json!({"asked": "2026-02-28T09:00:00Z"}),
        ),
        (
            json!({"asked": "2026-10-01T24:00:00Z"}),
            json!({"asked": "2026-10-01T00:00:00Z"}),
        ),
        (
            json!({"asked": "2026-10-01T09:00:00+24:00"}),
            json!({"asked": "2026-10-01T09:00:00+23:59"}),
        ),
        (
            json!({"asked": 20_261_001}),
            json!({"asked": "2026-10-01T09:00:00.123456789Z"}),
        ),
        (json!({"question": 1}), json!({"question": ""})),
        (json!({"session": null}), json!({"session": "local_0a1b"})),
        (
            json!({"recommendation": 1}),
            json!({"recommendation": null}),
        ),
        (json!({"blocks": ["release"]}), json!({"blocks": "release"})),
        (options(json!("a")), options(json!([]))),
        (json!({"options": Value::Null}), json!({})),
        (
            options(json!([{"id": "a", "text": "A", "why": "w"}])),
            options(json!([{"id": "a", "text": "A"}])),
        ),
        (
            options(json!([{"id": "", "text": "A"}])),
            options(json!([{"id": "a", "text": "A"}])),
        ),
        (
            options(json!([{"id": "a"}])),
            options(json!([{"text": "A", "id": "a"}])),
        ),
        (
            options(json!([["a", "A"]])),
            options(json!([{"id": "a", "text": "A"}])),
        ),
    ]
}

#[test]
fn a_misshapen_question_is_unreadable_and_its_well_formed_twin_is_read() {
    for (misshapen, twin) in twins() {
        let read = |fields: &Value| {
            let body = body("2026-10-01T09:00:00Z", fields);
            let (paths, output) = opened(vec![row("/q/a", None, body)], &json!({}));
            (paths.len(), output["unreadable"].as_u64().unwrap())
        };
        assert_eq!(read(&misshapen), (0, 1), "{misshapen}");
        assert_eq!(read(&twin), (1, 0), "{twin}");
    }
}

#[test]
fn a_body_that_is_not_an_object_is_unreadable_and_an_object_is_read() {
    let array = json!(["Which way?", "local_0a1b", "2026-10-01T09:00:00Z"]).to_string();
    for (body, unreadable) in [
        (Value::String(array), 1),
        (Value::Null, 1),
        (body("2026-10-01T09:00:00Z", &json!({})), 0),
    ] {
        let (_, output) = opened(vec![row("/q/a", None, body)], &json!({}));
        assert_eq!(output["unreadable"], unreadable);
    }
}

#[test]
fn a_question_is_returned_with_its_original_time_and_null_for_what_it_leaves_out() {
    let fields = json!({"options": [{"text": "A", "id": "a"}], "recommendation": "a", "repo": "r"});
    let rows = vec![row(
        "/q/a",
        None,
        body("2026-10-01t09:00:00.5+01:00", &fields),
    )];
    let (_, output) = opened(rows, &json!({}));
    assert_eq!(
        output["questions"][0],
        json!({
            "path": "/q/a",
            "question": "Which way?",
            "asked": "2026-10-01t09:00:00.5+01:00",
            "session": "local_0a1b",
            "options": [{"id": "a", "text": "A"}],
            "recommendation": "a",
            "blocks": null,
        })
    );
}

/// A board that enables plans, built in, holding `questions`.
fn plans_board(questions: &[(&str, Value)]) -> (Scenario, Board) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    board
        .plugin_enable(None, None, &"plans".parse().unwrap())
        .unwrap();
    for (path, body) in questions {
        scenario.write(&format!("{path}.json"), Item::File(body.as_str().unwrap()));
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    (scenario, board)
}

#[test]
fn a_failing_query_is_refused_with_boards_code_and_the_same_host_answering_is_read() {
    let (_scenario, board) = plans_board(&[]);
    let expected = (board.query("SELECT logical FROM nowhere", 1, None, None))
        .unwrap_err()
        .refusal();
    let mut failing = host(Vec::new());
    failing.failing = Some(board);
    let refused = open(&failing, &json!({})).unwrap_err();
    assert_eq!(refused.code, "plans::board::refused");
    assert_eq!(refused.fields["code"], json!(expected.code.as_str()));
    assert!(open(&host(Vec::new()), &json!({})).is_ok());
}

#[test]
fn board_calls_plans_open_in_its_own_process_over_the_boards_records() {
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/late",
            body("2026-10-02T09:00:00Z", &json!({})),
        ),
        (
            "projects/a/questions/early",
            body("2026-10-01T09:00:00Z", &json!({})),
        ),
        (
            "projects/a/questions/done",
            body("2026-09-01T09:00:00Z", &json!({"status": "answered"})),
        ),
        (
            "projects/b/questions/other",
            body("2026-09-01T09:00:00Z", &json!({})),
        ),
    ]);
    let target: Target = "plans.open".parse().unwrap();
    let report = board
        .call(&target, br#"{"path": "/projects/a"}"#, &Plugins::default())
        .unwrap();
    let paths: Vec<&str> = (report.output["questions"].as_array().unwrap().iter())
        .map(|question| question["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["/projects/a/questions/early", "/projects/a/questions/late"]
    );
    let trace = board.take_trace();
    assert_eq!(trace.work.get("started"), Some(&0));
    let phases: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
    assert!(!phases.contains(&"plugin.reply"), "{phases:?}");
    assert_eq!(Plans.schemas().len(), 3);
}

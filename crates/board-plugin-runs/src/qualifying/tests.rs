//! A run is read as the Python plugin's pydantic model read it, and the
//! runs that qualify are a suite's own full runs of a clean revision,
//! newest first.

use std::cell::RefCell;

use board_model::LogicalPath;
use board_ops::{
    Absent, Board, GetReport, Plugins, QueryError, ReadError, RefsError, RefsReport, Settings,
};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::*;
use crate::Runs;

/// A host that passes each query to a real board and records it.
pub(crate) struct Real {
    board: Board,
    pub(crate) settings: Settings,
    asked: RefCell<Vec<Limit>>,
    /// Whether each query names a table that does not exist, so Board refuses it.
    failing: bool,
}

impl Host for Real {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        self.asked.borrow_mut().push(limit);
        let sql = if self.failing {
            "SELECT logical FROM nowhere"
        } else {
            sql
        };
        self.board.query(sql, usize::MAX, None, None)
    }

    fn refs(
        &self,
        _paths: &[LogicalPath],
        _absent: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("this plugin resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        panic!("this plugin reads no files")
    }

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("this plugin writes no records")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

/// A board that enables runs, built in, holding `files` by path.
pub(crate) fn built(files: &[(&str, String)]) -> (Scenario, Board) {
    let scenario = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    scenario.write(".board/board.json", Item::File(config));
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Runs]);
    (board.plugin_enable(None, None, &"runs".parse().unwrap(), &[])).unwrap();
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Runs]);
    (scenario, board)
}

/// The output of `board call runs.<capability>` with `input` on a board
/// holding `files`, which Board checks against the capability's schemas, and
/// whether the call started a process.
pub(crate) fn called(files: &[(&str, String)], capability: &str, input: &Value) -> (Value, bool) {
    let (_scenario, board) = built(files);
    let target: Target = format!("runs.{capability}").parse().unwrap();
    let report = (board.call(&target, input.to_string().as_bytes(), &Plugins::default())).unwrap();
    let started = board.take_trace().work.get("started").copied();
    (report.output, started != Some(0))
}

/// A board that enables runs, built in, holding `files` by path, and a host
/// over it.
pub(crate) fn board(files: &[(&str, String)]) -> (Scenario, Real) {
    let (scenario, _) = built(files);
    let host = Real {
        board: Board::open(scenario.workdir(), None).unwrap(),
        settings: Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            caller: None,
        },
        asked: RefCell::default(),
        failing: false,
    };
    (scenario, host)
}

/// A `runs/suite/v1` record.
pub(crate) fn suite() -> String {
    json!({"$type": "runs/suite/v1", "subjects": ["board"], "method": "Run it."}).to_string()
}

/// A full run of a clean revision of `/suites/a` that started at
/// `started`, with `fields` added or replacing its own.
pub(crate) fn run(started: &str, fields: &Value) -> String {
    let mut record = json!({
        "$type": "runs/run/v1",
        "suite": "/suites/a",
        "by": "claude",
        "started": started,
        "scope": "full",
        "dirty": false,
        "results": [],
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

fn read(fields: &Value) -> Option<bool> {
    readable(&run("2026-10-01T09:00:00Z", fields)).map(|(_, clean)| clean)
}

#[test]
fn a_run_the_python_model_refused_is_unreadable_and_its_twin_is_read() {
    let result = |fields: Value| json!({"results": [fields]});
    for (refused, admitted) in [
        (
            json!({"started": "2026-10-01T09:00:00"}),
            json!({"started": "2026-10-01T09:00:00Z"}),
        ),
        (
            json!({"finished": "2026-10-01"}),
            json!({"finished": "2026-10-01T09:05:00Z"}),
        ),
        (json!({"scope": "partial"}), json!({"scope": "targeted"})),
        (json!({"by": 5}), json!({"by": "5"})),
        (json!({"suite": null}), json!({"suite": "/suites/b"})),
        (json!({"results": null}), json!({"results": []})),
        (
            json!({"versions": {"gix": 1}}),
            json!({"versions": {"gix": "1"}}),
        ),
        (json!({"setup": 5}), json!({"setup": "M4"})),
        (json!({"summary": []}), json!({"summary": "Passed."})),
        (
            json!({"written_by": "runs"}),
            json!({"written_by": "runs/hook"}),
        ),
        (
            json!({"written_by": "Runs/hook"}),
            json!({"written_by": "runs/hook-2"}),
        ),
        (
            json!({"written_by": "runs/a/b"}),
            json!({"written_by": "runs/a"}),
        ),
        (
            json!({"measures": {"s": null}}),
            json!({"measures": {"s": 1}}),
        ),
        (
            json!({"measures": {"s": [1]}}),
            json!({"measures": {"s": "fast"}}),
        ),
        (
            json!({"host": {"os": "macos", "arch": "aarch64", "logical_cpus": 0}}),
            json!({"host": {"os": "macos", "arch": "aarch64", "logical_cpus": 2.0}}),
        ),
        (
            json!({"host": {"os": "macos", "arch": "aarch64"}}),
            json!({"host": {"os": "macos", "arch": "aarch64", "logical_cpus": 1, "gpu": "m4"}}),
        ),
        (
            result(json!({"case": "a"})),
            result(json!({"case": "a", "measures": {}})),
        ),
        (
            result(json!({"outcome": "pass"})),
            result(json!({"case": "a", "outcome": "pass"})),
        ),
        (
            result(json!({"case": "a", "outcome": "fail"})),
            result(json!({"case": "a", "outcome": "fail", "note": "n"})),
        ),
        (
            result(json!({"case": "a", "outcome": "skipped"})),
            result(json!({"case": "a", "outcome": "skipped", "note": "n"})),
        ),
        (
            result(json!({"case": "a", "outcome": "partial"})),
            result(json!({"case": "a", "outcome": "error", "note": "n"})),
        ),
        (
            result(json!({"case": "a", "outcome": "pass", "trial": 0})),
            result(json!({"case": "a", "outcome": "pass", "trial": 1.0})),
        ),
        (
            result(json!({"case": "a", "outcome": "pass", "trial": 1.5})),
            result(json!({"case": "a", "outcome": "pass", "trial": 2})),
        ),
        (
            result(json!({"case": "a", "outcome": "pass", "evidence": [1]})),
            result(json!({"case": "a", "outcome": "pass", "evidence": ["/e"]})),
        ),
        (
            result(json!({"case": "a", "measures": {"s": {}}})),
            result(json!({"case": "a", "measures": {"s": true}})),
        ),
        // Pydantic read `"false"`, 0 and 1 as flags. A shipped schema admits
        // none of them, so only a looser schema of a board's own lets one
        // through, and the plugin no longer reads such a run.
        (json!({"dirty": "false"}), json!({"dirty": false})),
    ] {
        assert_eq!(read(&refused), None, "{refused}");
        assert!(read(&admitted).is_some(), "{admitted}");
    }
}

#[test]
fn a_run_is_clean_only_when_full_and_it_says_it_is_not_dirty() {
    assert_eq!(read(&json!({})), Some(true));
    assert_eq!(read(&json!({"scope": "targeted"})), Some(false));
    assert_eq!(read(&json!({"dirty": true})), Some(false));
    assert_eq!(read(&json!({"dirty": null})), Some(false));
    assert_eq!(read(&json!({"unknown": 1})), Some(true));
}

#[test]
fn a_suite_that_does_not_exist_is_refused_naming_it_and_one_that_does_is_answered() {
    let (scenario, host) = board(&[(
        "suites/a/runs/1.json",
        run("2026-10-01T09:00:00Z", &json!({})),
    )]);
    let Err(refused) = qualifying(&host, "/suites/a") else {
        panic!("no suite is at /suites/a")
    };
    assert_eq!(refused.code, "runs::suite::not_found");
    assert_eq!(
        refused.message,
        "no runs/suite/v1 record is at /suites/a; name a suite by its logical path, without .json, \
         and list the suites with `board query \"SELECT logical FROM records WHERE type = \
         'runs/suite/v1'\"`"
    );
    assert_eq!(
        refused.fields,
        Map::from_iter([("suite".to_owned(), json!("/suites/a"))])
    );
    scenario.write("suites/a.json", Item::File(&suite()));
    let host = Real {
        board: Board::open(scenario.workdir(), None).unwrap(),
        settings: host.settings,
        asked: RefCell::default(),
        failing: false,
    };
    let counted = qualifying(&host, "/suites/a").unwrap();
    assert_eq!(counted.runs.len(), 1);
    assert_eq!(
        *host.asked.borrow(),
        [Limit::ALL; 2],
        "each query reads every row"
    );
}

#[test]
fn runs_are_newest_first_by_their_instant_and_a_tie_goes_to_the_last_path() {
    let (_scenario, host) = board(&[
        ("suites/a.json", suite()),
        ("suites/ab.json", suite()),
        (
            "suites/a/runs/early.json",
            run("2026-10-02T10:00:00+02:00", &json!({})),
        ),
        (
            "suites/a/runs/late.json",
            run("2026-10-02T09:00:00Z", &json!({})),
        ),
        (
            "suites/a/runs/same-a.json",
            run("2026-10-01T09:00:00Z", &json!({})),
        ),
        (
            "suites/a/runs/same-b.json",
            run("2026-10-01T11:00:00+02:00", &json!({})),
        ),
        (
            "suites/ab/runs/other.json",
            run("2026-10-09T09:00:00Z", &json!({"suite": "/suites/ab"})),
        ),
        (
            "suites/a/runs/dirty.json",
            run("2026-10-03T09:00:00Z", &json!({"dirty": true})),
        ),
        (
            "suites/a/runs/invalid.json",
            run("2026-10-03T09:00:00Z", &json!({"scope": "partial"})),
        ),
    ]);
    let counted = qualifying(&host, "/suites/a").unwrap();
    let paths: Vec<&str> = counted.runs.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/suites/a/runs/late",
            "/suites/a/runs/early",
            "/suites/a/runs/same-b",
            "/suites/a/runs/same-a"
        ]
    );
    assert_eq!((counted.not_counted, counted.unreadable), (1, 1));
}

#[test]
fn a_query_board_refuses_is_refused_with_boards_code_and_the_same_query_answered_is_read() {
    let (_scenario, mut host) = board(&[("suites/a.json", suite())]);
    host.failing = true;
    let Err(refused) = qualifying(&host, "/suites/a") else {
        panic!("the query names no table")
    };
    assert_eq!(refused.code, "runs::board::refused");
    assert_eq!(refused.fields["code"], json!("board::query::invalid"));
    host.failing = false;
    assert!(qualifying(&host, "/suites/a").is_ok());
}

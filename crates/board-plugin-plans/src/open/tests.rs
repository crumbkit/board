//! `open` answers as the Python plugin's `open.py` did: the same readable
//! rule, the same order and the same counts.

use std::cell::RefCell;

use board_model::LogicalPath;
use board_ops::{Board, Expect, Plugin, Plugins, QueryReport, Settings};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::*;
use crate::Plans;

/// A host that answers every query with `rows` and records what it was
/// asked, or answers with `failing`'s refusal of a query that names no
/// table. With `board`, it queries that board and puts there instead, after
/// running `between`, and runs `before_read` before the first query that
/// reads one question by `question::question_sql`. It records each put's
/// lines either way.
pub(crate) struct Answering {
    rows: Vec<Vec<Value>>,
    pub(crate) asked: RefCell<Vec<(String, Limit)>>,
    failing: Option<Board>,
    pub(crate) board: Option<Board>,
    pub(crate) between: RefCell<Option<Box<dyn FnOnce()>>>,
    pub(crate) before_read: RefCell<Option<Box<dyn FnOnce()>>>,
    pub(crate) puts: RefCell<Vec<Vec<Value>>>,
    /// Each call's target and input; a call answers with `call_refusal`
    /// when it is set and with `{}` when not.
    pub(crate) calls: RefCell<Vec<(String, Value)>>,
    pub(crate) call_refusal: Option<board_model::Refusal>,
    pub(crate) settings: Settings,
}

impl Host for Answering {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, board_ops::QueryError> {
        self.asked.borrow_mut().push((sql.to_owned(), limit));
        if let Some(board) = &self.failing {
            board.query("SELECT logical FROM nowhere", 1, None, None)?;
        }
        if let Some(board) = &self.board {
            if sql.starts_with("SELECT 'question' AS kind")
                && let Some(before_read) = self.before_read.take()
            {
                before_read();
            }
            return board.query(sql, usize::MAX, None, None);
        }
        Ok(QueryReport {
            columns: vec!["logical".into(), "code".into(), "body".into()],
            rows: self.rows.clone(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("this plugin resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("this plugin reads no files")
    }

    fn put(
        &self,
        lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        self.puts.borrow_mut().push(lines.to_vec());
        if let Some(between) = self.between.take() {
            between();
        }
        let Some(board) = &self.board else {
            return Ok(board_ops::PutReport {
                writes: Vec::new(),
                removed: Vec::new(),
                absent: Vec::new(),
                commit: None,
                mended: Vec::new(),
            });
        };
        let text = (lines.iter().map(Value::to_string))
            .collect::<Vec<_>>()
            .join("\n");
        board.put_batch(&mut text.as_bytes())
    }

    fn call(
        &self,
        target: &board_plugin::Target,
        input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        let Target::Named { plugin, capability } = target else {
            panic!("this plugin names the plugin it calls")
        };
        (self.calls.borrow_mut()).push((
            format!("{}.{}", plugin.as_str(), capability.as_str()),
            input.clone(),
        ));
        if let Some(refusal) = &self.call_refusal {
            return Err(Box::new(refusal.clone()));
        }
        Ok(board_plugin::CallReport {
            plugin: plugin.clone(),
            capability: capability.clone(),
            output: json!({}),
            warnings: Vec::new(),
        })
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("this plugin opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("this plugin asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("this host syncs nothing")
    }
    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("this host updates no mount")
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
}

pub(crate) fn host(rows: Vec<Vec<Value>>) -> Answering {
    Answering {
        rows,
        asked: RefCell::default(),
        failing: None,
        board: None,
        between: RefCell::default(),
        before_read: RefCell::default(),
        puts: RefCell::default(),
        calls: RefCell::default(),
        call_refusal: None,
        settings: Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            utc_offset: 0,
            caller: None,
            environment: board_ops::Variables::default(),
            cache_home: None,
        },
    }
}

/// A `plans/question/v1` body asked at `asked`, with `fields` added or
/// replacing its own.
pub(crate) fn body(asked: &str, fields: &Value) -> Value {
    let mut record = json!({
        "$type": "plans/question/v1",
        "question": "Which way?",
        "by": {"session": "local_0a1b"},
        "asked": asked,
        "status": "open",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    Value::String(record.to_string())
}

/// A row of the question at `path` with `code` and `body`, whose `blocks`
/// names no work the query found.
fn row(path: &str, code: Option<&str>, body: Value) -> Vec<Value> {
    vec![json!(path), json!(code), body, Value::Null]
}

/// A row of the open question at `path`, asked on day `day` of October, whose
/// `blocks` names work in `work`: `active`, `done`, `dropped` or null.
fn worked(path: &str, day: u8, work: Option<&str>) -> Vec<Value> {
    let asked = format!("2026-10-{day:02}T09:00:00Z");
    vec![
        json!(path),
        Value::Null,
        body(&asked, &json!({})),
        json!(work),
    ]
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
    assert_eq!(host.asked.take(), [(questions(), Limit::ALL)]);
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
        .into_iter()
        .chain([vec![
            Value::Null,
            Value::Null,
            body("2026-10-01T09:00:00Z", &json!({})),
        ]])
        .collect::<Vec<_>>()
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
    let (paths, output) = opened(rows(), &json!({}));
    assert_eq!(paths.len(), 3, "no path is every question");
    assert_eq!(
        (&output["total"], &output["unreadable"]),
        (&json!(3), &json!(0)),
        "a row without a logical path is neither listed nor unreadable"
    );
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
        (
            json!({"by": {"program": "codex"}}),
            json!({"by": {"session": "local_0a1b"}}),
        ),
        (
            json!({"recommendation": 1}),
            json!({"recommendation": null}),
        ),
        (
            json!({"blocks": ["release"]}),
            json!({"blocks": "/changes/release"}),
        ),
        (json!({"why": 1}), json!({"why": "It is simpler."})),
        (
            json!({"note": false}),
            json!({"note": "Nobody wrote it down."}),
        ),
        (json!({"mount": ["/repos/r"]}), json!({"mount": "/repos/r"})),
        (
            json!({"context_path": {}}),
            json!({"context_path": "/projects/a/notes/background"}),
        ),
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
    let fields =
        json!({"options": [{"text": "A", "id": "a"}], "recommendation": "a", "mount": "/repos/r"});
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
            "also_asked": [],
            "options": [{"id": "a", "text": "A"}],
            "recommendation": "a",
            "why": null,
            "blocks": null,
            "blocked_work": null,
            "note": null,
            "mount": "/repos/r",
            "context_path": null,
            "project": "/q",
            "critical": false,
        })
    );
}

/// A question a person asked without a session is listed with `person` and
/// no `session`. Its twin, asked by a program alone, names nobody the answer
/// can reach and stays unreadable.
#[test]
fn a_question_a_person_asked_is_listed_without_a_session_and_a_programs_is_unreadable() {
    let read = |by: Value| {
        let rows = vec![row(
            "/q/a",
            None,
            body("2026-10-01T09:00:00Z", &json!({"by": by})),
        )];
        opened(rows, &json!({})).1
    };
    let output = read(json!({"person": "ana"}));
    let question = &output["questions"][0];
    assert_eq!(question["person"], json!("ana"));
    assert_eq!(question.get("session"), None, "{question}");
    let output = read(json!({"program": "codex"}));
    assert_eq!(
        (output["total"].as_u64(), output["unreadable"].as_u64()),
        (Some(0), Some(1))
    );
}

#[test]
fn a_question_is_returned_with_the_reasoning_and_background_it_carries() {
    let fields = json!({
        "options": [{"id": "a", "text": "A"}],
        "recommendation": "a",
        "why": "It is simpler.",
        "note": "Nobody wrote down the deadline.",
        "mount": "/repos/r",
        "context_path": "/projects/a/notes/background.md",
    });
    let rows = vec![row("/q/a", None, body("2026-10-01T09:00:00Z", &fields))];
    let (_, output) = opened(rows, &json!({}));
    let question = &output["questions"][0];
    assert_eq!(
        (
            &question["why"],
            &question["note"],
            &question["mount"],
            &question["context_path"]
        ),
        (
            &json!("It is simpler."),
            &json!("Nobody wrote down the deadline."),
            &json!("/repos/r"),
            &json!("/projects/a/notes/background.md"),
        )
    );
}

/// A board that enables plans, built in, holding `questions`.
pub(crate) fn plans_board(questions: &[(&str, Value)]) -> (Scenario, Board) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    board
        .plugin_enable(None, None, &"plans".parse().unwrap(), &[])
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
fn board_calls_plans_open_in_its_own_process_and_leaves_out_answered_and_withdrawn_questions() {
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
            body("2026-09-01T09:00:00Z", &json!({})),
        ),
        (
            "projects/a/questions/dropped",
            body(
                "2026-09-01T09:00:00Z",
                &json!({"status": "withdrawn", "withdrawn_reason": "Stale."}),
            ),
        ),
        (
            "projects/a/answers/elsewhere",
            Value::String(
                json!({
                    "$type": "plans/answer/v1",
                    "question": "/projects/a/questions/done",
                    "words": "Done already.",
                    "answered": "2026-09-02T09:00:00Z",
                })
                .to_string(),
            ),
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

/// A board about the size of the owner's personal board: 900 questions in
/// 20 projects, 780 of them answered, so that 120 stay open. Half of the
/// answers sit beside their question and half in another project.
fn busy_board() -> (Scenario, Board) {
    let mut files = Vec::new();
    for number in 0..900 {
        let project = number % 20;
        let asked = format!("2026-10-{:02}T09:00:00Z", 1 + number % 28);
        files.push((
            format!("projects/p{project}/questions/q{number}"),
            body(&asked, &json!({})),
        ));
        if number < 780 {
            let home = if number % 2 == 0 {
                project
            } else {
                (project + 1) % 20
            };
            files.push((
                format!("projects/p{home}/answers/q{number}"),
                Value::String(
                    json!({
                        "$type": "plans/answer/v1",
                        "question": format!("/projects/p{project}/questions/q{number}"),
                        "words": "Done.",
                        "answered": "2026-10-30T09:00:00Z",
                    })
                    .to_string(),
                ),
            ));
        }
    }
    let files: Vec<(&str, Value)> = files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
    plans_board(&files)
}

/// The answer record that names `question`.
pub(crate) fn answer_naming(question: &str) -> Value {
    Value::String(
        json!({
            "$type": "plans/answer/v1",
            "question": question,
            "words": "Done.",
            "answered": "2026-10-30T09:00:00Z",
        })
        .to_string(),
    )
}

/// A plans board holding `root`, and a board mounted at `/vendor` that
/// enables plans and holds `mounted`. The paths of `mounted` are its own, so
/// `projects/a/questions/q` there is `/vendor/projects/a/questions/q` here.
pub(crate) fn mounting_board(
    root: &[(&str, Value)],
    mounted: &[(&str, Value)],
) -> (Scenario, Scenario, Board) {
    let (scenario, _) = plans_board(&[]);
    // The root board commits each put too, so a test can count its commits.
    let config = scenario.workdir().join(".board/board.json");
    let mut root_config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    root_config.as_object_mut().unwrap().remove("commit_on_put");
    (scenario.write(".board/board.json", Item::File(&root_config.to_string()))).identified();
    let schemas = scenario.workdir().join(".board/types/plans");
    let library = Scenario::new();
    let files: Vec<(String, String)> = (mounted.iter())
        .map(|(path, body)| (format!("{path}.json"), body.as_str().unwrap().to_owned()))
        .collect();
    let mut files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let config = r#"{"$type": "board/config/v1", "plugins": ["plans"]}"#;
    files.push((".board/board.json", config));
    let schema_files: Vec<(String, String)> = ["answer", "question", "withdraw"]
        .iter()
        .filter_map(|name| {
            let path = format!(".board/types/plans/{name}/v1.schema.json");
            let text = std::fs::read_to_string(schemas.join(format!("{name}/v1.schema.json")));
            text.ok().map(|text| (path, text))
        })
        .collect();
    files.extend(schema_files.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    let pinned = library.commit(&files);
    // A write through the mount needs the library's working tree to hold
    // what it commits (docs/architecture/writes.md). It also needs a git
    // identity to commit as, because Board refuses a mounted board that sets
    // `commit_on_put` to false, and CI has no global identity to fall back on.
    for (path, text) in &files {
        library.write(path, Item::File(text));
    }
    library.identified();
    let mount = json!({"$type": "board/mount/v1", "repo": library.path(), "follow": "main", "commit": pinned.to_string()});
    scenario.write("vendor.json", Item::File(&mount.to_string()));
    for (path, body) in root {
        scenario.write(&format!("{path}.json"), Item::File(body.as_str().unwrap()));
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    (scenario, library, board)
}

fn open_paths(board: &Board) -> Vec<String> {
    let report = board.query(&questions(), usize::MAX, None, None).unwrap();
    let mut paths: Vec<String> = (report.rows.iter())
        .map(|row| row[0].as_str().unwrap().to_owned())
        .collect();
    paths.sort();
    paths
}

pub(crate) const MOUNTED_QUESTION: &str = "projects/a/questions/q";

/// An answer in a mounted board names a question there by its path in that
/// board, which a reference in a mounted board is read from. The twin is the
/// same answer in the root board, which is another source: it names the
/// question only by its logical path, which the mount puts under `/vendor`.
#[test]
fn an_answer_in_a_mounted_board_names_its_question_by_its_own_path_and_one_from_another_source_does_not()
 {
    let asked = body("2026-10-01T09:00:00Z", &json!({}));
    let own = answer_naming("/projects/a/questions/q");
    let (_s, _l, board) = mounting_board(&[], &[(MOUNTED_QUESTION, asked.clone())]);
    assert_eq!(open_paths(&board), ["/vendor/projects/a/questions/q"]);
    let (_s, _l, board) = mounting_board(
        &[],
        &[
            (MOUNTED_QUESTION, asked.clone()),
            ("projects/a/answers/q", own.clone()),
        ],
    );
    assert_eq!(open_paths(&board), Vec::<String>::new());
    let (_s, _l, board) = mounting_board(
        &[("projects/a/answers/q", own)],
        &[(MOUNTED_QUESTION, asked.clone())],
    );
    assert_eq!(open_paths(&board), ["/vendor/projects/a/questions/q"]);
    let (_s, _l, board) = mounting_board(
        &[(
            "projects/a/answers/q",
            answer_naming("/vendor/projects/a/questions/q"),
        )],
        &[(MOUNTED_QUESTION, asked)],
    );
    assert_eq!(open_paths(&board), Vec::<String>::new());
}

/// `NAMED` and `NAMES` are two spellings of one fact. Over answers in the
/// root board and in a mounted board, naming each question by its logical or
/// its own path, the questions each leaves unnamed are the same.
#[test]
fn the_open_query_leaves_out_the_questions_that_the_names_predicate_says_an_answer_names() {
    let asked = body("2026-10-01T09:00:00Z", &json!({}));
    let paths = [
        "/projects/a/questions/q",
        "/vendor/projects/a/questions/q",
        "/projects/a/questions/none",
    ];
    let mut checked = 0;
    for (in_root, in_mount) in [
        (None, None),
        (Some(0), None),
        (Some(1), None),
        (None, Some(0)),
        (None, Some(1)),
        (Some(0), Some(1)),
        (Some(1), Some(0)),
    ] {
        let root: Vec<(&str, Value)> = [("projects/a/questions/q", asked.clone())]
            .into_iter()
            .chain(in_root.map(|n| ("projects/a/answers/q", answer_naming(paths[n]))))
            .collect();
        let mounted: Vec<(&str, Value)> = [(MOUNTED_QUESTION, asked.clone())]
            .into_iter()
            .chain(in_mount.map(|n| ("projects/a/answers/q", answer_naming(paths[n]))))
            .collect();
        let (_s, _l, board) = mounting_board(&root, &mounted);
        let unnamed = format!(
            "SELECT q.logical FROM records q WHERE q.type = 'plans/question/v1' \
             AND NOT EXISTS (SELECT 1 FROM records a WHERE a.type = 'plans/answer/v1' AND {})",
            crate::question::NAMES
        );
        let report = board.query(&unnamed, usize::MAX, None, None).unwrap();
        let mut by_names: Vec<String> = (report.rows.iter())
            .map(|row| row[0].as_str().unwrap().to_owned())
            .collect();
        by_names.sort();
        assert_eq!(open_paths(&board), by_names, "{in_root:?} {in_mount:?}");
        checked += by_names.len();
    }
    assert!(checked > 0);
}

/// What each step of the query's plan does, as SQLite reports it.
fn plan(board: &Board) -> Vec<String> {
    let sql = format!("EXPLAIN QUERY PLAN {}", questions());
    let report = board.query(&sql, usize::MAX, None, None).unwrap();
    let column = report.columns.iter().position(|c| c == "detail").unwrap();
    (report.rows.iter())
        .map(|row| row[column].as_str().unwrap().to_owned())
        .collect()
}

/// Each question is looked up in `named` by key. A step that scans `named`
/// for every question compares every question with every answer, which ran
/// past the one second a query may take on a board of 9,000 questions and
/// 4,500 answers. Counting the plan's steps does not depend on the machine.
/// The twin shows that `plan` reports the scans of a query that has them.
#[test]
fn plans_open_looks_up_each_named_question_by_key_and_never_scans_the_answers_per_question() {
    let (_scenario, board) = busy_board();
    let scans = |steps: &[String]| {
        (steps.iter())
            .filter(|step| step.starts_with("SCAN n"))
            .count()
    };
    let steps = plan(&board);
    assert_eq!(scans(&steps), 0, "{steps:#?}");
    // The work a question blocks is looked up by its logical path, never by
    // scanning every record for each question.
    assert!(
        !steps.iter().any(|step| step.starts_with("SCAN w")),
        "{steps:#?}"
    );
    let searches: Vec<&String> = (steps.iter())
        .filter(|step| step.starts_with("SEARCH n"))
        .collect();
    // The two sources and the own path make three lookups, each by `target`.
    assert_eq!(searches.len(), 3, "{steps:#?}");
    for search in searches {
        assert!(search.contains("target=?"), "{steps:#?}");
    }
    let unkeyed = "EXPLAIN QUERY PLAN WITH named AS MATERIALIZED (SELECT a.source AS source, \
         json_extract(a.body, '$.question') AS target FROM records a \
         WHERE a.type = 'plans/answer/v1') SELECT q.logical FROM records q \
         WHERE q.type = 'plans/question/v1' \
         AND NOT EXISTS (SELECT 1 FROM named n WHERE n.target || '' = q.logical)";
    let report = board.query(unkeyed, usize::MAX, None, None).unwrap();
    let column = report.columns.iter().position(|c| c == "detail").unwrap();
    let steps: Vec<String> = (report.rows.iter())
        .map(|row| row[column].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(scans(&steps), 1, "{steps:#?}");
}

/// The question list of a board the size of the personal board lists the
/// questions no answer names. The query once compared every question with
/// every answer, parsing each answer's body again each time, and ran past the
/// one second a query may take on the real personal board.
#[test]
fn plans_open_on_a_board_with_hundreds_of_questions_and_answers_lists_the_unanswered() {
    let (_scenario, board) = busy_board();
    let report = board.query(&questions(), usize::MAX, None, None).unwrap();
    assert_eq!(report.rows.len(), 120);
    let target: Target = "plans.open".parse().unwrap();
    let report = board
        .call(&target, br#"{"limit": 100}"#, &Plugins::default())
        .unwrap();
    assert_eq!(report.output["total"], json!(120));
}

/// Within a project, a question that holds up active work comes first and
/// the rest follow oldest first. A question whose work ended, or whose
/// `blocks` names nothing the query found, is not critical.
#[test]
fn within_a_project_questions_blocking_active_work_come_first_then_the_rest_oldest_first() {
    let rows = vec![
        worked("/projects/a/questions/old", 1, None),
        worked("/projects/a/questions/ended", 2, Some("done")),
        worked("/projects/a/questions/late-critical", 4, Some("active")),
        worked("/projects/a/questions/early-critical", 3, Some("active")),
    ];
    let (paths, output) = opened(rows, &json!({}));
    assert_eq!(
        paths,
        [
            "/projects/a/questions/early-critical",
            "/projects/a/questions/late-critical",
            "/projects/a/questions/old",
            "/projects/a/questions/ended",
        ]
    );
    let critical: Vec<&Value> = (output["questions"].as_array().unwrap().iter())
        .map(|question| &question["critical"])
        .collect();
    assert_eq!(
        critical,
        [&json!(true), &json!(true), &json!(false), &json!(false)]
    );
}

/// Questions stay together by project. A project holding a critical
/// question comes before one holding none, however old that one's
/// questions are, and projects of the same kind come oldest first.
#[test]
fn projects_with_critical_questions_come_first_and_each_projects_questions_stay_together() {
    let rows = vec![
        worked("/projects/calm/questions/oldest", 1, None),
        worked("/projects/late/questions/blocking", 9, Some("active")),
        worked("/projects/early/questions/later", 8, None),
        worked("/projects/early/questions/blocking", 5, Some("active")),
        worked("/projects/late/questions/other", 2, None),
        worked("/projects/calm/questions/newer", 3, None),
        worked("/projects/quiet/questions/only", 2, None),
    ];
    let (paths, output) = opened(rows, &json!({}));
    assert_eq!(
        paths,
        [
            "/projects/early/questions/blocking",
            "/projects/early/questions/later",
            "/projects/late/questions/blocking",
            "/projects/late/questions/other",
            "/projects/calm/questions/oldest",
            "/projects/calm/questions/newer",
            "/projects/quiet/questions/only",
        ]
    );
    let projects: Vec<&str> = (output["questions"].as_array().unwrap().iter())
        .map(|question| question["project"].as_str().unwrap())
        .collect();
    assert_eq!(projects[0], "/projects/early");
}

/// The limit keeps the first questions of the digest order, so a critical
/// question asked last is still returned when the limit is one.
#[test]
fn the_limit_keeps_the_most_urgent_questions_of_the_digest_order() {
    let rows = vec![
        worked("/projects/a/questions/old", 1, None),
        worked("/projects/b/questions/new", 9, Some("active")),
    ];
    let (paths, output) = opened(rows, &json!({"limit": 1}));
    assert_eq!(paths, ["/projects/b/questions/new"]);
    assert_eq!(
        (&output["total"], &output["truncated"]),
        (&json!(2), &json!(true))
    );
}

/// A question's project is the folder that holds its `questions/` folder,
/// so a mounted board's project keeps the mount's prefix.
#[test]
fn a_questions_project_is_the_folder_above_its_questions_folder() {
    let rows = vec![
        worked("/repos/lib/projects/demos/questions/q", 1, None),
        worked("/elsewhere/q", 2, None),
        worked("/questions/q", 3, None),
        worked("/q", 4, None),
        worked("/projects/a/questions/sub/q", 5, None),
    ];
    let (_, output) = opened(rows, &json!({}));
    let projects: Vec<&Value> = (output["questions"].as_array().unwrap().iter())
        .map(|question| &question["project"])
        .collect();
    assert_eq!(
        projects,
        [
            &json!("/repos/lib/projects/demos"),
            &json!("/elsewhere"),
            &json!("/"),
            &json!("/"),
            &json!("/projects/a/questions/sub"),
        ]
    );
}

/// A change record in `phase`, as the changes plugin writes one.
pub(crate) fn change(phase: &str) -> Value {
    Value::String(
        json!({"$type": "changes/change/v2", "title": "Work", "phase": phase}).to_string(),
    )
}

/// The work column the query reads for each open question, by path.
fn work_of(board: &Board) -> Vec<(String, Value)> {
    let report = board.query(&questions(), usize::MAX, None, None).unwrap();
    let mut work: Vec<(String, Value)> = (report.rows.iter())
        .map(|row| (row[0].as_str().unwrap().to_owned(), row[3].clone()))
        .collect();
    work.sort_by(|a, b| a.0.cmp(&b.0));
    work
}

/// The query reads the phase of the change a question blocks: `done` and
/// `dropped` are given as they are and every other phase is active. A `blocks` that
/// names nothing gives null.
#[test]
fn the_query_reads_a_blocked_change_as_active_until_it_is_done_or_dropped() {
    let mut files = vec![];
    for phase in [
        "plan", "build", "review", "land", "paused", "done", "dropped",
    ] {
        files.push((format!("changes/{phase}"), change(phase)));
        files.push((
            format!("projects/a/questions/{phase}"),
            body(
                "2026-10-01T09:00:00Z",
                &json!({"blocks": format!("/changes/{phase}")}),
            ),
        ));
    }
    files.push((
        "projects/a/questions/nothing".to_owned(),
        body(
            "2026-10-01T09:00:00Z",
            &json!({"blocks": "/changes/missing"}),
        ),
    ));
    files.push((
        "projects/a/questions/unblocking".to_owned(),
        body("2026-10-01T09:00:00Z", &json!({})),
    ));
    let files: Vec<(&str, Value)> = files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
    let (_scenario, board) = plans_board(&files);
    let work = work_of(&board);
    let expected = [
        ("build", json!("active")),
        ("done", json!("done")),
        ("dropped", json!("dropped")),
        ("land", json!("active")),
        ("nothing", Value::Null),
        ("paused", json!("active")),
        ("plan", json!("active")),
        ("review", json!("active")),
        ("unblocking", Value::Null),
    ];
    let expected: Vec<(String, Value)> = (expected.into_iter())
        .map(|(name, work)| (format!("/projects/a/questions/{name}"), work))
        .collect();
    assert_eq!(work, expected);
}

/// A task is a Markdown file whose frontmatter is the record. An open task
/// is active, and a done or dropped one is given as it is.
#[test]
fn the_query_reads_a_blocked_task_as_active_while_it_is_open() {
    let (scenario, _) = plans_board(&[]);
    for status in ["open", "done", "dropped"] {
        let task =
            format!("---\n$type: tasks/task/v2\ntitle: Work\nstatus: {status}\n---\nBody.\n");
        scenario.write(&format!("tasks/{status}.md"), Item::File(&task));
        let question = body(
            "2026-10-01T09:00:00Z",
            &json!({"blocks": format!("/tasks/{status}.md")}),
        );
        scenario.write(
            &format!("projects/a/questions/{status}.json"),
            Item::File(question.as_str().unwrap()),
        );
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    let work = work_of(&board);
    assert_eq!(
        work,
        [
            ("/projects/a/questions/done".to_owned(), json!("done")),
            ("/projects/a/questions/dropped".to_owned(), json!("dropped")),
            ("/projects/a/questions/open".to_owned(), json!("active")),
        ]
    );
}

/// A question in a mounted board names the change it blocks by its path in
/// that board, so the query finds the change under the mount. The twin is
/// the same `blocks` on a question in the root board, which finds no
/// change there.
#[test]
fn a_mounted_questions_blocks_is_read_from_its_own_boards_root() {
    let question = body("2026-10-01T09:00:00Z", &json!({"blocks": "/changes/w"}));
    let (_s, _l, board) = mounting_board(
        &[("projects/a/questions/root", question.clone())],
        &[(MOUNTED_QUESTION, question), ("changes/w", change("build"))],
    );
    assert_eq!(
        work_of(&board),
        [
            ("/projects/a/questions/root".to_owned(), Value::Null),
            ("/vendor/projects/a/questions/q".to_owned(), json!("active")),
        ]
    );
}

/// A blocked change with no phase, which Board marks degraded but still
/// indexes with its type, gives the question null work: neither critical
/// nor ended. Its twin blocks a change with a phase, which is active.
#[test]
fn a_question_blocking_a_change_with_no_phase_has_no_work_and_its_twin_with_a_phase_is_active() {
    let blocking = |change: &str| {
        body(
            "2026-10-01T09:00:00Z",
            &json!({"blocks": format!("/changes/{change}")}),
        )
    };
    let broken = Value::String(json!({"$type": "changes/change/v2", "title": "Work"}).to_string());
    let (_scenario, board) = plans_board(&[
        ("changes/broken", broken),
        ("changes/whole", change("build")),
        ("projects/a/questions/broken", blocking("broken")),
        ("projects/a/questions/whole", blocking("whole")),
    ]);
    assert_eq!(
        work_of(&board),
        [
            ("/projects/a/questions/broken".to_owned(), Value::Null),
            ("/projects/a/questions/whole".to_owned(), json!("active")),
        ]
    );
    let target: Target = "plans.open".parse().unwrap();
    let report = board.call(&target, b"{}", &Plugins::default()).unwrap();
    let critical: Vec<(&Value, &Value)> = (report.output["questions"].as_array().unwrap().iter())
        .map(|question| (&question["path"], &question["critical"]))
        .collect();
    assert_eq!(
        critical,
        [
            (&json!("/projects/a/questions/whole"), &json!(true)),
            (&json!("/projects/a/questions/broken"), &json!(false)),
        ]
    );
}

/// `put` writes a question whose `blocks` names a task of either version whole. Its twin,
/// whose `blocks` names a record that is neither a change nor a task, is
/// written too, as Board writes every broken reference, but marked degraded
/// with `board::ref::wrong_type` at `/blocks`.
#[test]
fn put_writes_a_question_blocking_a_task_of_either_version_whole_and_marks_its_twin_blocking_another_type()
 {
    let (scenario, _) = plans_board(&[(
        "projects/a/questions/other",
        body("2026-10-01T09:00:00Z", &json!({})),
    )]);
    scenario.write(
        "tasks/w.md",
        Item::File("---\n$type: tasks/task/v2\ntitle: Work\nstatus: open\n---\nBody.\n"),
    );
    scenario.write(
        "tasks/v3.md",
        Item::File("---\n$type: tasks/task/v3\ntitle: Work\nstatus: open\n---\nBody.\n"),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Plans]);
    let degraded = |blocks: &str| {
        let path: LogicalPath = "/projects/a/questions/q".parse().unwrap();
        let question = body("2026-10-01T09:00:00Z", &json!({"blocks": blocks}));
        let report =
            (board.put(&path, Expect::Any, question.as_str().unwrap().as_bytes())).unwrap();
        (report.writes[0].degraded.as_ref())
            .map(|(code, detail)| (code.as_str(), detail.pointer.clone()))
    };
    assert_eq!(
        degraded("/projects/a/questions/other"),
        Some(("board::ref::wrong_type", Some("/blocks".to_owned())))
    );
    assert_eq!(degraded("/tasks/w.md"), None);
    assert_eq!(degraded("/tasks/v3.md"), None);
}

//! `about` and `open` answer as the Python plugin's SQL and pydantic model
//! did: the same rows, the same readable rule, order and counts.

use std::cell::RefCell;

use board_ops::{Board, Plugins, QueryReport, Settings};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use proptest::prelude::*;
use serde_json::json;

use super::*;
use crate::Evals;

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
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("this plugin writes no records")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("this plugin calls no plugin")
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

fn host(rows: Vec<Vec<Value>>) -> Answering {
    Answering {
        rows,
        asked: RefCell::default(),
        failing: None,
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

/// An `evals/eval/v1` record about gix, planned, with `fields` added or
/// replacing its own.
fn record(fields: &Value) -> Value {
    let mut record = json!({
        "$type": "evals/eval/v1",
        "question": "Is gix faster?",
        "subjects": ["o/gix"],
        "status": "planned",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record
}

/// A row of the eval at `path` with `code` and the record with `fields`.
fn row(path: &str, code: Option<&str>, fields: &Value) -> Vec<Value> {
    vec![json!(path), json!(code), json!(record(fields).to_string())]
}

/// The paths a capability lists from `rows` for `input`, and its output.
fn listed(
    capability: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
    rows: Vec<Vec<Value>>,
    input: &Value,
) -> (Vec<String>, Value) {
    let output = capability(&host(rows), input).unwrap();
    let paths = output["evals"].as_array().unwrap().iter();
    let paths = paths.map(|eval| eval["path"].as_str().unwrap().to_owned());
    (paths.collect(), output)
}

/// Each capability asks for every eval once with no row limit, and `about`
/// by subject also asks for every topic once.
#[test]
fn each_capability_asks_for_every_eval_once_with_no_row_limit() {
    for capability in [about, open] {
        let host = host(Vec::new());
        capability(&host, &json!({"subject": "o/gix"})).unwrap_or_default();
        capability(&host, &json!({})).unwrap_or_default();
        let asked = host.asked.take();
        assert_ne!(asked, Vec::new());
        assert!(
            asked.iter().all(|asked| [EVALS, TOPICS, SOURCES]
                .iter()
                .any(|sql| *asked == ((*sql).to_owned(), Limit::ALL))),
            "{asked:?}"
        );
    }
    let host = host(Vec::new());
    about(&host, &json!({"subject": "o/gix"})).unwrap();
    let asked: Vec<String> = host.asked.take().into_iter().map(|(sql, _)| sql).collect();
    assert_eq!(asked, [TOPICS, EVALS]);
}

#[test]
fn open_lists_planned_and_running_evals_and_leaves_out_concluded_and_abandoned_ones() {
    let rows = vec![
        row("/evals/a", None, &json!({"status": "planned"})),
        row(
            "/evals/b",
            None,
            &json!({"status": "concluded", "verdict": "refuted"}),
        ),
        row("/evals/c", None, &json!({"status": "running"})),
        row("/evals/d", None, &json!({"status": "abandoned"})),
    ];
    let (paths, output) = listed(open, rows, &json!({}));
    assert_eq!(paths, ["/evals/a", "/evals/c"]);
    assert_eq!(
        output["evals"][0],
        json!({
            "path": "/evals/a",
            "question": "Is gix faster?",
            "subjects": ["o/gix"],
            "status": "planned",
            "verdict": null,
            "decision": null,
        })
    );
}

#[test]
fn a_subject_matches_ignoring_ascii_case_only_and_percent_and_underscore_match_only_themselves() {
    let rows = vec![
        row(
            "/evals/gix",
            None,
            &json!({"subjects": ["o/clap", "o/gix"]}),
        ),
        row("/evals/a", None, &json!({"subjects": ["a"]})),
        row("/evals/umlaut", None, &json!({"subjects": ["ä"]})),
        row("/evals/percent", None, &json!({"subjects": ["%"]})),
        row("/evals/under", None, &json!({"subjects": ["_"]})),
    ];
    for (subject, expected) in [
        ("O/GIX", &["/evals/gix"][..]),
        ("A", &["/evals/a"]),
        ("Ä", &[]),
        ("ä", &["/evals/umlaut"]),
        ("%", &["/evals/percent"]),
        ("_", &["/evals/under"]),
        ("g_x", &[]),
    ] {
        let (paths, _) = listed(about, rows.clone(), &json!({"subject": subject}));
        assert_eq!(paths, expected, "{subject}");
    }
}

#[test]
fn a_mount_matches_an_eval_naming_it_exactly_or_living_below_it_and_no_other() {
    let rows = vec![
        row("/evals/names", None, &json!({"mounts": ["/repos/a"]})),
        row("/evals/names-ab", None, &json!({"mounts": ["/repos/ab"]})),
        row("/repos/a/evals/x", None, &json!({})),
        row("/repos/ab/evals/x", None, &json!({})),
    ];
    for (mount, expected) in [
        ("/repos/a", &["/evals/names", "/repos/a/evals/x"][..]),
        ("/repos/ab", &["/evals/names-ab", "/repos/ab/evals/x"]),
        ("/repos/a/", &[]),
        ("/repos/%", &[]),
    ] {
        let (paths, _) = listed(about, rows.clone(), &json!({"mount": mount}));
        assert_eq!(paths, expected, "{mount}");
    }
    let both = json!({"mount": "/repos/ab", "subject": "o/gix"});
    assert_eq!(
        listed(about, rows, &both).0,
        ["/evals/names-ab", "/repos/ab/evals/x"]
    );
}

#[test]
fn the_limit_cuts_the_evals_but_not_the_total_and_says_it_cut() {
    let rows = vec![
        row("/evals/a", None, &json!({})),
        row("/evals/b", None, &json!({})),
        row("/evals/c", None, &json!({})),
    ];
    for (limit, kept, truncated) in [(2.0, 2, true), (3.0, 3, false)] {
        let (paths, output) = listed(open, rows.clone(), &json!({"limit": limit}));
        assert_eq!(paths.len(), kept);
        assert_eq!(
            (&output["total"], &output["truncated"]),
            (&json!(3), &json!(truncated))
        );
    }
}

#[test]
fn an_eval_degraded_by_a_reference_is_listed_and_one_degraded_by_its_own_content_is_not() {
    let rows = vec![
        row("/evals/clean", None, &json!({})),
        row("/evals/dangling", Some("board::ref::dangling"), &json!({})),
        row("/evals/wrong", Some("board::ref::wrong_type"), &json!({})),
        row("/evals/invalid", Some("board::record::invalid"), &json!({})),
    ];
    let (paths, output) = listed(open, rows, &json!({}));
    assert_eq!(paths, ["/evals/clean", "/evals/dangling", "/evals/wrong"]);
    assert_eq!(output["total"], 3);
}

/// Each misshapen field beside a well-formed twin of the same field.
fn twins() -> Vec<(Value, Value)> {
    vec![
        (json!({"question": null}), json!({"question": ""})),
        (json!({"subjects": []}), json!({"subjects": ["o/gix"]})),
        (
            json!({"subjects": ["o/gix", 5]}),
            json!({"subjects": ["o/gix", "5"]}),
        ),
        (json!({"subjects": "o/gix"}), json!({"subjects": ["o/gix"]})),
        (json!({"status": 5}), json!({"status": "running"})),
        (json!({"verdict": 5}), json!({"verdict": "refuted"})),
        (
            json!({"status": "concluded"}),
            json!({"status": "concluded", "verdict": "supported"}),
        ),
        (json!({"decision": 5}), json!({"decision": "Pick."})),
        (json!({"conclusion": true}), json!({"conclusion": "Yes."})),
        (
            json!({"mounts": "/repos/a"}),
            json!({"mounts": ["/repos/a"]}),
        ),
        (json!({"suites": [1]}), json!({"suites": ["/suites/s"]})),
        (json!({"runs": {}}), json!({"runs": []})),
        (
            json!({"written_by": "Runs/test-run"}),
            json!({"written_by": "runs/test-run"}),
        ),
        (
            json!({"written_by": "runs"}),
            json!({"written_by": "runs/t"}),
        ),
        (
            json!({"written_by": "runs/test/run"}),
            json!({"written_by": "runs/test-run"}),
        ),
        (
            json!({"written_by": "runs/test-run\n"}),
            json!({"written_by": "runs/test-run"}),
        ),
        (
            json!({"written_by": "/run"}),
            json!({"written_by": "r/run"}),
        ),
        (json!({"written_by": ""}), json!({"written_by": "r/r"})),
    ]
}

#[test]
fn a_misshapen_eval_is_left_out_and_its_well_formed_twin_is_listed() {
    for (misshapen, twin) in twins() {
        let read = |fields: &Value| {
            let rows = vec![row("/evals/e", None, fields)];
            listed(about, rows, &json!({"subject": "O/GIX"})).1["total"].clone()
        };
        assert_eq!(read(&misshapen), 0, "{misshapen}");
        assert_eq!(read(&twin), 1, "{twin}");
    }
}

#[test]
fn an_unknown_field_and_a_null_optional_field_are_listed_and_a_missing_question_is_not() {
    let rows = vec![
        row("/evals/extra", None, &json!({"colour": "red"})),
        row(
            "/evals/null",
            None,
            &json!({"decision": null, "mounts": null, "written_by": null}),
        ),
        vec![
            json!("/evals/bare"),
            Value::Null,
            json!(r#"{"subjects": ["o/gix"], "status": "planned"}"#),
        ],
        vec![json!("/evals/torn"), Value::Null, json!(r#"{"question": "#)],
    ];
    assert_eq!(
        listed(open, rows, &json!({})).0,
        ["/evals/extra", "/evals/null"]
    );
}

/// A board that enables evals, built in, holding `evals` and a mount of a
/// library at `/vendor` that holds one eval.
fn evals_board(evals: &[(&str, Value)]) -> (Scenario, Scenario, Board) {
    let library = record(&json!({"subjects": ["o/clap"]}));
    evals_board_with(evals, &library, &[])
}

/// As [`evals_board`], with `library` as the library's eval, and the
/// library's schema admitting each `(property, value)` of `newer` as well,
/// as a newer Board's schema would.
fn evals_board_with(
    evals: &[(&str, Value)],
    library_eval: &Value,
    newer: &[(&str, &str)],
) -> (Scenario, Scenario, Board) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Evals]);
    board
        .plugin_enable(None, None, &"evals".parse().unwrap(), &[])
        .unwrap();
    let schema = std::fs::read_to_string(
        scenario
            .workdir()
            .join(".board/types/evals/eval/v1.schema.json"),
    )
    .unwrap();
    let mut schema: Value = serde_json::from_str(&schema).unwrap();
    for (property, value) in newer {
        let values = schema["properties"][property]["enum"]
            .as_array_mut()
            .unwrap();
        values.push(json!(value));
    }
    let config = r#"{"$type": "board/config/v1", "plugins": ["evals"]}"#;
    let library = Scenario::new();
    let pinned = library.commit(&[
        (".board/board.json", config),
        (
            ".board/types/evals/eval/v1.schema.json",
            &schema.to_string(),
        ),
        ("evals/l.json", &library_eval.to_string()),
    ]);
    let mount = json!({"$type": "board/mount/v1", "repo": library.path(), "follow": "main", "commit": pinned.to_string()});
    scenario.write("vendor.json", Item::File(&mount.to_string()));
    for (path, record) in evals {
        scenario.write(&format!("{path}.json"), Item::File(&record.to_string()));
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Evals]);
    (scenario, library, board)
}

#[test]
fn a_failing_query_is_refused_with_boards_code_and_the_same_host_answering_is_read() {
    let (_scenario, _library, board) = evals_board(&[]);
    let expected = (board.query("SELECT logical FROM nowhere", 1, None, None))
        .unwrap_err()
        .refusal();
    let mut failing = host(Vec::new());
    failing.failing = Some(board);
    let refused = open(&failing, &json!({})).unwrap_err();
    assert_eq!(refused.code, "evals::board::refused");
    assert_eq!(refused.fields["code"], json!(expected.code.as_str()));
    assert!(open(&host(Vec::new()), &json!({})).is_ok());
}

#[test]
fn board_calls_evals_in_its_own_process_over_this_board_and_its_mounted_ones() {
    let (_scenario, _library, board) = evals_board(&[
        ("evals/b", record(&json!({"subjects": ["o/clap", "o/gix"]}))),
        (
            "evals/a",
            record(&json!({"status": "concluded", "verdict": "supported"})),
        ),
    ]);
    let call = |target: &str, input: &[u8]| {
        let target: Target = target.parse().unwrap();
        let report = board.call(&target, input, &Plugins::default()).unwrap();
        let paths = report.output["evals"].as_array().unwrap().iter();
        paths
            .map(|eval| eval["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        call("evals.about", br#"{"subject": "O/CLAP"}"#),
        ["/evals/b", "/vendor/evals/l"]
    );
    assert_eq!(
        call("evals.about", br#"{"mount": "/vendor"}"#),
        ["/vendor/evals/l"]
    );
    assert_eq!(call("evals.open", b"{}"), ["/evals/b", "/vendor/evals/l"]);
    let trace = board.take_trace();
    assert_eq!(trace.work.get("started"), Some(&0));
    let phases: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
    assert!(!phases.contains(&"plugin.reply"), "{phases:?}");
}

/// A mounted board a newer Board wrote can hold a status and a verdict this
/// Board does not know. `about` and `open` list that eval with both as
/// written, as they list its twin holding known values; `open` lists it
/// because it is not known to be concluded or abandoned.
#[test]
fn an_eval_holding_a_status_and_verdict_a_newer_board_added_is_listed_as_written_like_its_twin() {
    for (status, verdict) in [("running", "refuted"), ("paused", "mixed")] {
        let library =
            record(&json!({"subjects": ["o/clap"], "status": status, "verdict": verdict}));
        let newer = [("status", "paused"), ("verdict", "mixed")];
        let (_scenario, _library, board) = evals_board_with(&[], &library, &newer);
        for (name, input) in [
            ("evals.about", r#"{"mount": "/vendor"}"#),
            ("evals.open", "{}"),
        ] {
            let target: Target = name.parse().unwrap();
            let report = (board.call(&target, input.as_bytes(), &Plugins::default())).unwrap();
            let listed = &report.output["evals"];
            assert_eq!(listed.as_array().unwrap().len(), 1, "{name}: {listed}");
            assert_eq!(listed[0]["path"], "/vendor/evals/l");
            assert_eq!(listed[0]["status"], status);
            assert_eq!(listed[0]["verdict"], verdict);
        }
    }
}

/// Any string without NUL, which neither an argument nor a logical path can
/// carry, drawn mostly from letters whose case folds differ in Unicode.
fn text() -> impl Strategy<Value = String> {
    let folding = prop::sample::select(vec!['a', 'A', 'z', 'Z', 'ä', 'Ä', 'İ', 'ı', 'ß', '%', '_']);
    let character = prop_oneof![3 => folding, 1 => any::<char>()];
    proptest::collection::vec(character.prop_filter("no NUL", |c| *c != '\0'), 0..6)
        .prop_map(|chars| chars.into_iter().collect())
}

thread_local! {
    /// An empty board, whose query runs SQL in Board's bundled SQLite.
    static EMPTY: (Scenario, Board) = {
        let scenario = Scenario::new();
        scenario.write(".board/board.json", Item::File(r#"{"$type": "board/config/v1"}"#));
        let board = Board::open(scenario.workdir(), None).unwrap();
        (scenario, board)
    };
}

/// `text` as an SQL string literal.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Whether Board's `SQLite` finds the comparison `sql` true.
fn sqlite_says(sql: &str) -> bool {
    EMPTY.with(|(_, board)| {
        let report = board
            .query(&format!("SELECT {sql}"), 1, None, None)
            .unwrap();
        report.rows[0][0] == json!(1)
    })
}

proptest! {
    #[test]
    fn ascii_case_folding_equals_sqlite_lower(a in text(), b in text()) {
        let equal = sqlite_says(&format!("lower({}) = lower({})", literal(&a), literal(&b)));
        prop_assert_eq!(a.eq_ignore_ascii_case(&b), equal);
    }

    #[test]
    fn starts_with_equals_sqlite_substr_and_length(a in text(), b in text()) {
        let (a_sql, b_sql) = (literal(&a), literal(&b));
        let equal = sqlite_says(&format!("substr({a_sql}, 1, length({b_sql})) = {b_sql}"));
        prop_assert_eq!(a.starts_with(&b), equal);
    }
}

/// A board that enables evals and topics, built in, holding `records` by
/// path, and a mounted board that holds a topic and an eval naming it.
fn topics_board(records: &[(&str, Value)]) -> (Scenario, Scenario, Board) {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Evals, &board_plugin_topics::Topics]);
    for plugin in ["evals", "topics"] {
        (board.plugin_enable(None, None, &plugin.parse().unwrap(), &[])).unwrap();
    }
    let read = |path: &str| std::fs::read_to_string(scenario.workdir().join(path)).unwrap();
    let (eval, topic) = (
        read(".board/types/evals/eval/v1.schema.json"),
        read(".board/types/topics/topic/v1.schema.json"),
    );
    let library = Scenario::new();
    let seatbelt = json!({"$type": "topics/topic/v1", "title": "seatbelt"}).to_string();
    let pinned = library.commit(&[
        (
            ".board/board.json",
            r#"{"$type": "board/config/v1", "plugins": ["evals", "topics"]}"#,
        ),
        (".board/types/evals/eval/v1.schema.json", &eval),
        (".board/types/topics/topic/v1.schema.json", &topic),
        ("topics/seatbelt.json", &seatbelt),
        (
            "evals/l.json",
            &record(&json!({"subjects": ["/topics/seatbelt"]})).to_string(),
        ),
    ]);
    let mount = json!({"$type": "board/mount/v1", "repo": library.path(), "follow": "main", "commit": pinned.to_string()});
    scenario.write("vendor.json", Item::File(&mount.to_string()));
    for (path, record) in records {
        scenario.write(&format!("{path}.json"), Item::File(&record.to_string()));
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Evals, &board_plugin_topics::Topics]);
    (scenario, library, board)
}

/// `about` finds an eval whose subject is a topic by the topic's title or
/// any alias, ignoring case, and a topic in a mounted board only for that
/// board's evals.
#[test]
fn about_finds_an_eval_naming_a_topic_by_the_topics_title_or_alias_and_only_in_its_own_board() {
    let topic =
        json!({"$type": "topics/topic/v1", "title": "seatbelt", "aliases": ["sandbox-exec"]});
    let (_scenario, _library, board) = topics_board(&[
        ("topics/seatbelt", topic),
        (
            "evals/a",
            record(&json!({"subjects": ["/topics/seatbelt"]})),
        ),
        (
            "evals/b",
            record(&json!({"subjects": ["/topics/landlock"]})),
        ),
    ]);
    let about = |subject: &str| {
        let target: Target = "evals.about".parse().unwrap();
        let input = json!({ "subject": subject }).to_string();
        let report = board
            .call(&target, input.as_bytes(), &Plugins::default())
            .unwrap();
        (report.output["evals"].as_array().unwrap().iter())
            .map(|eval| eval["path"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let both = ["/evals/a", "/vendor/evals/l"];
    assert_eq!(about("Seatbelt"), both);
    assert_eq!(about("SANDBOX-EXEC"), ["/evals/a"]);
    assert_eq!(about("/topics/seatbelt"), both);
    assert_eq!(about("landlock"), Vec::<String>::new());
}

/// A bare name matches an `owner/name` subject by its name, as it did
/// before subjects named repositories, and never a board path's last part.
#[test]
fn a_bare_name_matches_a_repository_subject_by_its_name_and_not_a_paths_last_part() {
    let rows = vec![
        row("/evals/a", None, &json!({"subjects": ["clap-rs/clap"]})),
        row("/evals/b", None, &json!({"subjects": ["/repos/clap"]})),
        row("/evals/c", None, &json!({"subjects": ["o/clapper"]})),
    ];
    let (paths, _) = listed(about, rows.clone(), &json!({"subject": "CLAP"}));
    assert_eq!(paths, ["/evals/a"]);
    let (paths, _) = listed(about, rows, &json!({"subject": "clap-rs/clap"}));
    assert_eq!(paths, ["/evals/a"]);
}

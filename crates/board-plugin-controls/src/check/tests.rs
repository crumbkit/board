//! The work `check` and `for-path` ask of Board, counted over a host that
//! records each call it passes on to a real board, and the rules `check`
//! measures by.

use std::cell::RefCell;

use board_model::LogicalPath;
use board_ops::{
    Board, GetReport, Limit, QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings,
};
use board_scenario::{Item, Scenario};

use super::*;
use crate::Controls;
use crate::for_path::for_path;

/// One call the plugin made of its host, with the paths it named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Query,
    Refs(Vec<String>),
    GetEach(Vec<String>),
}

/// A host that passes each call to `board` and records it.
pub(crate) struct Recording {
    board: Board,
    settings: Settings,
    calls: RefCell<Vec<Call>>,
}

impl Recording {
    /// How many queries were made since the last time calls were taken.
    fn queries(&self) -> usize {
        (self.calls.borrow().iter())
            .filter(|call| **call == Call::Query)
            .count()
    }

    /// The `refs` and `get_each` calls made since the last time this was asked.
    pub(crate) fn resolved(&self) -> Vec<Call> {
        (self.calls.take().into_iter())
            .filter(|call| *call != Call::Query)
            .collect()
    }
}

fn named(paths: &[LogicalPath]) -> Vec<String> {
    paths.iter().map(ToString::to_string).collect()
}

impl Host for Recording {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        assert_eq!(limit, Limit::ALL, "the plugin reads every row");
        self.calls.borrow_mut().push(Call::Query);
        self.board.query(sql, usize::MAX, None, None)
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        self.calls.borrow_mut().push(Call::Refs(named(paths)));
        Ok(self.board.refs(paths, absent, &[], None, None)?.collect())
    }

    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        self.calls.borrow_mut().push(Call::GetEach(named(paths)));
        Ok(self.board.get_each(paths)?.collect())
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

/// A board that enables controls, built in, holding `files` by path, and a
/// host recording the calls made of it.
pub(crate) fn recording(files: &[(&str, &str)]) -> (Scenario, Recording) {
    let scenario = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    scenario.write(".board/board.json", Item::File(config));
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Controls]);
    (board.plugin_enable(None, None, &"controls".parse().unwrap(), &[])).unwrap();
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    let board = Board::open(scenario.workdir(), None).unwrap();
    let host = Recording {
        board,
        settings: Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            utc_offset: 0,
            caller: None,
            environment: board_ops::Variables::default(),
            cache_home: None,
        },
        calls: RefCell::default(),
    };
    (scenario, host)
}

/// A control with `fields` added to its requirement and purpose.
fn control(fields: &Value) -> String {
    let mut record = json!({"$type": "controls/control/v1", "requirement": "R.", "purpose": "P."});
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

fn words(max: u64) -> Value {
    json!({"check": "budget", "unit": "words", "max": max})
}

#[test]
fn check_resolves_every_controls_references_in_one_refs_call_and_makes_none_when_no_control_has_any()
 {
    let a = control(&json!({"applies_to": ["/docs/a.md"], "rule": words(9)}));
    let b = control(&json!({"applies_to": ["/docs/a.md", "/docs/b.md"]}));
    let pattern = control(&json!({"paths": ["docs/*"], "rule": words(9)}));
    let docs = [("docs/a.md", "one two"), ("docs/b.md", "three")];
    let (_scenario, host) = recording(&[
        docs[0],
        docs[1],
        ("controls/b.json", &b),
        ("controls/a.json", &a),
        ("controls/pattern.json", &pattern),
    ]);
    check(&host, &json!({})).unwrap();
    assert_eq!(
        host.queries(),
        4,
        "degraded controls, controls, matched files and mounts"
    );
    let refs: Vec<Call> = (host.resolved().into_iter())
        .filter(|call| matches!(call, Call::Refs(_)))
        .collect();
    assert_eq!(
        refs,
        [Call::Refs(vec!["/controls/a".into(), "/controls/b".into()])]
    );
    let (_scenario, host) = recording(&[docs[0], docs[1], ("controls/pattern.json", &pattern)]);
    check(&host, &json!({})).unwrap();
    assert_eq!(
        host.resolved(),
        [Call::GetEach(vec![
            "/docs/a.md".into(),
            "/docs/b.md".into()
        ])]
    );
}

#[test]
fn check_reads_each_file_once_in_one_get_each_call_however_many_controls_reach_it() {
    let docs = [
        ("docs/a.md", "one two"),
        ("docs/b.md", "three"),
        ("docs/c.md", "four"),
    ];
    let words_control = control(
        &json!({"applies_to": ["/alias/a.md", "/docs/b.md"], "paths": ["docs/*"], "rule": words(9)}),
    );
    let bytes_control = control(
        &json!({"applies_to": ["/docs/a.md", "/docs/c.md"], "rule": {"check": "budget", "unit": "bytes", "max": 99}}),
    );
    let (_scenario, host) = recording(&[
        docs[0],
        docs[1],
        docs[2],
        (
            "alias.json",
            r#"{"$type": "board/mount/v1", "path": "/docs"}"#,
        ),
        ("controls/words.json", &words_control),
        ("controls/bytes.json", &bytes_control),
    ]);
    let output = check(&host, &json!({})).unwrap();
    let used: Vec<&Value> = (output["measured"].as_array().unwrap().iter())
        .map(|item| &item["used"])
        .collect();
    assert_eq!(used, [&json!(11), &json!(4)]);
    let reads: Vec<Call> = (host.resolved().into_iter())
        .filter(|call| matches!(call, Call::GetEach(_)))
        .collect();
    let all = ["/docs/a.md", "/docs/b.md", "/docs/c.md"].map(String::from);
    assert_eq!(reads, [Call::GetEach(all.to_vec())]);
}

#[test]
fn check_with_no_budget_reads_no_file_and_with_one_reads_its_files() {
    let stated = control(&json!({"paths": ["docs/*"]}));
    let (_scenario, host) = recording(&[("docs/a.md", "one"), ("controls/stated.json", &stated)]);
    check(&host, &json!({})).unwrap();
    assert_eq!(host.resolved(), []);
    let budget = control(&json!({"paths": ["docs/*"], "rule": words(9)}));
    let (_scenario, host) = recording(&[("docs/a.md", "one"), ("controls/budget.json", &budget)]);
    check(&host, &json!({})).unwrap();
    assert_eq!(host.resolved(), [Call::GetEach(vec!["/docs/a.md".into()])]);
}

#[test]
fn for_path_makes_one_refs_call_and_reads_no_file() {
    let applies = control(&json!({"applies_to": ["/docs/a.md"], "rule": words(9)}));
    let pattern = control(&json!({"paths": ["docs/*"]}));
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("controls/applies.json", &applies),
        ("controls/pattern.json", &pattern),
    ]);
    let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
    assert_eq!(
        host.queries(),
        2,
        "controls and the patterns matching the path"
    );
    let controls: Vec<&Value> = (output["controls"].as_array().unwrap().iter())
        .map(|item| &item["control"])
        .collect();
    assert_eq!(
        controls,
        [&json!("/controls/applies"), &json!("/controls/pattern")]
    );
    assert_eq!(host.resolved(), [Call::Refs(vec!["/docs/a.md".into()])]);
}

#[test]
fn words_are_split_at_whitespace_as_python_splits_them_and_bytes_are_counted_whole() {
    for (text, expected) in [
        ("one two\nthree\n", 3),
        ("one\u{1c}two\u{1f}three", 3),
        ("one\u{a0}two\u{3000}three", 3),
        ("one\u{200b}two", 1),
        ("", 0),
    ] {
        assert_eq!(
            measure(text.as_bytes(), Measure::Words),
            expected,
            "{text:?}"
        );
    }
    assert_eq!(measure(b"one \xff two", Measure::Words), 3);
    assert_eq!(measure("é".as_bytes(), Measure::Bytes), 2);
}

#[test]
fn a_path_is_under_itself_and_its_directories_and_not_under_a_sibling_that_shares_its_prefix() {
    let under = |path: &str, scope: &str| under(&path.parse().unwrap(), &scope.parse().unwrap());
    assert!(under("/docs/a.md", "/"));
    assert!(under("/docs/a.md", "/docs"));
    assert!(under("/docs", "/docs"));
    assert!(!under("/docs2/a.md", "/docs"));
    assert!(!under("/doc", "/docs"));
}

#[test]
fn a_source_that_names_nothing_degrades_its_control_and_one_naming_a_source_note_passes() {
    let note = json!({
        "$type": "controls/source/v1",
        "url": "https://example.com/paper",
        "title": "A paper.",
        "read_on": "2026-11-01",
        "why": "It measures the cost.",
    });
    let citing = |source: &str| control(&json!({"paths": ["docs/*"], "sources": [source]}));
    let dangling = citing("/controls/sources/missing");
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("controls/sources/paper.json", &note.to_string()),
        ("controls/cites.json", &dangling),
    ]);
    let refused = check(&host, &json!({})).unwrap_err();
    assert_eq!(refused.code, "controls::control::degraded");
    assert_eq!(
        refused.fields["controls"],
        json!([{"control": "/controls/cites", "code": "board::ref::dangling"}])
    );
    let cited = citing("/controls/sources/paper");
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("controls/sources/paper.json", &note.to_string()),
        ("controls/cites.json", &cited),
    ]);
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(output["stated"][0]["control"], "/controls/cites");
}

/// Six controls on `docs/a.md`: an obligation, a budget and a stated
/// control that leave `show` out, and a twin of each that sets it.
fn shown() -> (Scenario, Recording) {
    let on_docs = |mut fields: Value, show: Option<&str>| {
        fields["paths"] = json!(["docs/*"]);
        if let Some(show) = show {
            fields["show"] = json!(show);
        }
        control(&fields)
    };
    let obligation = |show| on_docs(json!({"kind": "obligation"}), show);
    let budget = |show| on_docs(json!({"rule": words(9)}), show);
    let stated = |show| on_docs(json!({"kind": "standing"}), show);
    recording(&[
        ("docs/a.md", "one"),
        ("controls/a-obligation.json", &obligation(None)),
        ("controls/b-obligation-edit.json", &obligation(Some("edit"))),
        ("controls/c-budget.json", &budget(None)),
        ("controls/d-budget-start.json", &budget(Some("start"))),
        ("controls/e-stated.json", &stated(None)),
        ("controls/f-stated-failing.json", &stated(Some("failing"))),
    ])
}

/// Each item's control, kind and show.
fn kinds(items: &Value) -> Vec<(String, String, String)> {
    (items.as_array().unwrap().iter())
        .map(|item| {
            let field = |name: &str| item[name].as_str().unwrap().to_owned();
            (field("control"), field("kind"), field("show"))
        })
        .collect()
}

fn row(control: &str, kind: &str, show: &str) -> (String, String, String) {
    (
        format!("/controls/{control}"),
        kind.to_owned(),
        show.to_owned(),
    )
}

#[test]
fn check_and_for_path_give_each_control_its_kind_and_the_show_it_defaults_to_or_sets() {
    let (_scenario, host) = shown();
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(
        kinds(&output["measured"]),
        [
            row("c-budget", "standing", "failing"),
            row("d-budget-start", "standing", "start"),
        ]
    );
    let stated = [
        row("a-obligation", "obligation", "start"),
        row("b-obligation-edit", "obligation", "edit"),
        row("e-stated", "standing", "edit"),
        row("f-stated-failing", "standing", "failing"),
    ];
    assert_eq!(kinds(&output["stated"]), stated);
    let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
    assert_eq!(
        kinds(&output["controls"]),
        [
            stated[0].clone(),
            stated[1].clone(),
            row("c-budget", "standing", "failing"),
            row("d-budget-start", "standing", "start"),
            stated[2].clone(),
            stated[3].clone(),
        ]
    );
}

#[test]
fn a_budget_over_its_max_returns_the_same_report_as_a_pass_with_passed_false() {
    let budget = |max: u64| control(&json!({"paths": ["docs/*"], "rule": words(max)}));
    let start = control(&json!({"paths": ["docs/*"], "show": "start"}));
    let files = |budget: &str| {
        recording(&[
            ("docs/a.md", "one two"),
            ("controls/budget.json", budget),
            ("controls/start.json", &start),
        ])
    };
    let (_scenario, host) = files(&budget(1));
    let report = &check(&host, &json!({})).unwrap();
    assert_eq!(report["passed"], false, "{report}");
    assert_eq!(report["measured"][0]["used"], 2);
    assert_eq!(
        kinds(&report["stated"]),
        [row("start", "standing", "start")]
    );
    let (_scenario, host) = files(&budget(2));
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(output["passed"], true, "{output}");
    assert_eq!(output["measured"][0]["used"], 2);
    assert_eq!(output["stated"], report["stated"]);
}

/// A board whose control schema a newer Board replaced, so it admits each
/// `(pointer, value)` of `newer` beside the values at that pointer's
/// `enum`, holding `files` by path.
fn newer(files: &[(&str, &str)], newer: &[(&str, &str)]) -> (Scenario, Recording) {
    let mut schema: Value =
        serde_json::from_str(include_str!("../../types/controls/control/v1.schema.json")).unwrap();
    for (pointer, value) in newer {
        let values = schema.pointer_mut(&format!("{pointer}/enum")).unwrap();
        values.as_array_mut().unwrap().push(json!(value));
    }
    let schema = schema.to_string();
    let mut files = files.to_vec();
    files.push((".board/types/controls/control/v1.schema.json", &schema));
    recording(&files)
}

/// A control whose kind a newer Board added reads as standing, as its
/// standing twin does, in `check` and `for-path`.
#[test]
fn a_kind_a_newer_board_added_reads_as_standing_like_its_standing_twin() {
    for kind in ["standing", "reminder"] {
        let fields = json!({"paths": ["docs/*"], "kind": kind});
        let (_scenario, host) = newer(
            &[("docs/a.md", "one"), ("controls/a.json", &control(&fields))],
            &[("/properties/kind", "reminder")],
        );
        let output = check(&host, &json!({})).unwrap();
        assert_eq!(
            kinds(&output["stated"]),
            [row("a", "standing", "edit")],
            "{kind}"
        );
        let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
        assert_eq!(
            kinds(&output["controls"]),
            [row("a", "standing", "edit")],
            "{kind}"
        );
    }
}

/// A control whose show a newer Board added is shown at the start, the
/// first point, as its twin that says `start` is.
#[test]
fn a_show_a_newer_board_added_reads_as_start_like_its_start_twin() {
    for show in ["start", "hourly"] {
        let fields = json!({"paths": ["docs/*"], "show": show});
        let (_scenario, host) = newer(
            &[("docs/a.md", "one"), ("controls/a.json", &control(&fields))],
            &[("/properties/show", "hourly")],
        );
        let output = check(&host, &json!({})).unwrap();
        assert_eq!(
            kinds(&output["stated"]),
            [row("a", "standing", "start")],
            "{show}"
        );
    }
}

/// A rule whose `check`, budget unit, `per` or pattern `quote` a newer
/// Board added is not applied: `check` states its control with its
/// requirement and measures nothing, and `evaluate` gives it `stated`. Its
/// twin with known values is measured and evaluated.
#[test]
fn a_rule_holding_a_value_a_newer_board_added_is_stated_and_its_twin_is_applied() {
    let pattern = |quote: &str| json!({"check": "pattern", "file": "/names.txt", "quote": quote});
    let budget =
        |unit: &str, per: &str| json!({"check": "budget", "unit": unit, "max": 9, "per": per});
    let newer_values = [
        ("/$defs/Rule/properties/check", "count"),
        ("/$defs/BudgetRule/properties/unit", "pages"),
        ("/$defs/BudgetRule/properties/per", "line"),
        ("/$defs/PatternRule/properties/quote", "redacted"),
    ];
    for (known, unknown, verdict) in [
        (words(9), json!({"check": "count"}), "budget"),
        (words(9), budget("pages", "total"), "budget"),
        (budget("words", "file"), budget("words", "line"), "budget"),
        (pattern("match-only"), pattern("redacted"), "pattern"),
    ] {
        let on_docs = |rule: &Value| control(&json!({"paths": ["docs/*"], "rule": rule}));
        let (_scenario, host) = newer(
            &[
                ("docs/a.md", "one"),
                ("names.txt", "acme\n"),
                ("controls/known.json", &on_docs(&known)),
                ("controls/unknown.json", &on_docs(&unknown)),
            ],
            &newer_values,
        );
        let output = check(&host, &json!({})).unwrap();
        let stated: Vec<&Value> = (output["stated"].as_array().unwrap().iter())
            .filter(|item| item["control"] == "/controls/unknown")
            .collect();
        assert_eq!(stated.len(), 1, "{unknown}: {output}");
        assert_eq!(stated[0]["requirement"], "R.");
        let measured = |items: &Value, control: &str| {
            (items.as_array().unwrap().iter()).any(|item| item["control"] == control)
        };
        assert!(
            !measured(&output["measured"], "/controls/unknown"),
            "{unknown}"
        );
        assert_eq!(
            measured(&output["measured"], "/controls/known"),
            verdict == "budget"
        );
        let input =
            json!({"controls": ["/controls/known", "/controls/unknown"], "parts": {"body": "one"}});
        let output = crate::evaluate::evaluate(&host, &input).unwrap();
        let results = &output["results"];
        assert_eq!(
            (&results[0]["rule"], &results[0]["status"]),
            (&json!(verdict), &json!("passed")),
            "{known}"
        );
        assert_eq!(
            (&results[1]["rule"], &results[1]["status"]),
            (&Value::Null, &json!("stated")),
            "{unknown}"
        );
        let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
        let rules: Vec<&Value> = (output["controls"].as_array().unwrap().iter())
            .map(|item| &item["rule"])
            .collect();
        assert_eq!(
            rules,
            [&known, &unknown],
            "for-path repeats each rule as held"
        );
    }
}

#[test]
fn read_names_the_own_tree_and_each_mount_the_files_came_from_and_a_scoped_check_only_its_mount() {
    let library = Scenario::new();
    let commit = library.commit(&[("a.md", "one two")]);
    let pointer = board_scenario::seed::mount_of(&library.path(), commit);
    let pattern = control(&json!({"paths": ["docs/*", "repos/lib/*"], "rule": words(9)}));
    let (_scenario, host) = recording(&[
        ("docs/a.md", "three"),
        ("repos/lib.json", &pointer),
        ("controls/pattern.json", &pattern),
    ]);
    let whole = check(&host, &json!({})).unwrap();
    assert_eq!(
        whole["read"],
        json!([
            {"mount": "/", "commit": null},
            {"mount": "/repos/lib", "commit": commit.to_string()},
        ])
    );
    let scoped = check(&host, &json!({"path": "/repos/lib"})).unwrap();
    assert_eq!(
        scoped["read"],
        json!([{"mount": "/repos/lib", "commit": commit.to_string()}])
    );
}

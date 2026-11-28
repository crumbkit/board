//! The work `check` and `for-path` ask of Board, counted over a host that
//! records each call it passes on to a real board, and the rules `check`
//! measures by.

use std::cell::RefCell;

use board_model::LogicalPath;
use board_ops::{
    Board, GetReport, Limit, Plugin, QueryError, QueryReport, ReadError, RefsError, RefsReport,
    Settings,
};
use board_scenario::{Item, Scenario};

use super::*;
use crate::Rules;
use crate::for_path::for_path;
use crate::for_paths::for_paths;

/// One call the plugin made of its host, with the paths it named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Query,
    Refs(Vec<String>),
    GetEach(Vec<String>),
    /// A call of another plugin's capability, by `<plugin>.<capability>`.
    Called(String),
}

/// A host that passes each call to `board` and records it.
pub(crate) struct Recording {
    board: Board,
    pub(crate) settings: Settings,
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

    /// Runs a capability of the runs plugin, built in, over this host, so
    /// it reads the same board at the same time.
    fn call(
        &self,
        target: &board_plugin::Target,
        input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        let board_plugin::Target::Named { plugin, capability } = target else {
            panic!("the plugin names the plugin it calls")
        };
        (self.calls.borrow_mut()).push(Call::Called(format!("{plugin}.{capability}")));
        assert_eq!(plugin.as_str(), "runs", "the plugin calls only runs");
        match board_plugin_runs::Runs.call(self, capability, input.clone()) {
            Ok(output) => Ok(board_plugin::CallReport {
                plugin: plugin.clone(),
                capability: capability.clone(),
                output,
                warnings: Vec::new(),
            }),
            Err(error) => Err(Box::new(board_model::Refusal::plugin(
                board_model::PluginCode::parse(plugin, error.code).unwrap(),
                error.message,
                error.fields,
            ))),
        }
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
    enabling(&["controls"], files)
}

/// A board that enables each of `plugins`, built in, holding `files` by
/// path, and a host recording the calls made of it.
pub(crate) fn enabling(plugins: &[&str], files: &[(&str, &str)]) -> (Scenario, Recording) {
    let scenario = Scenario::new();
    let host = enabling_in(&scenario, plugins, files);
    (scenario, host)
}

/// [`enabling`] over `scenario`, a repository that may already hold
/// commits.
pub(crate) fn enabling_in(
    scenario: &Scenario,
    plugins: &[&str],
    files: &[(&str, &str)],
) -> Recording {
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    scenario.write(".board/board.json", Item::File(config));
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[
        &Rules,
        &board_plugin_runs::Runs,
        &board_plugin_boundary::Boundary,
        &board_plugin_github::Github,
        &board_plugin_plans::Plans,
        &board_plugin_review::Review,
    ]);
    for plugin in plugins {
        (board.plugin_enable(None, None, &plugin.parse().unwrap(), &[])).unwrap();
    }
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    let board = Board::open(scenario.workdir(), None).unwrap();
    Recording {
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
    }
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
fn a_rule_of_either_type_is_checked_alike_and_one_with_no_checker_is_described() {
    let old = |mut fields: Value| {
        fields["paths"] = json!(["docs/*"]);
        control(&fields)
    };
    let rule = |fields: Value| old(fields).replace("controls/control/v1", "rules/rule/v1");
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one two"),
        ("rules/budget.json", &rule(json!({"checker": words(1)}))),
        ("rules/plain.json", &rule(json!({}))),
        ("controls/budget.json", &old(json!({"rule": words(1)}))),
        ("controls/plain.json", &old(json!({}))),
        (
            "controls/judged.json",
            &old(json!({"rule": {"check": "judged", "rubric": "/docs/a.md"}})),
        ),
    ]);
    let output = check(&host, &json!({})).unwrap();
    let over: Vec<(&Value, &Value)> = (output["measured"].as_array().unwrap().iter())
        .map(|each| (&each["rule"], &each["used"]))
        .collect();
    assert_eq!(
        over,
        [
            (&json!("/controls/budget"), &json!(2)),
            (&json!("/rules/budget"), &json!(2))
        ]
    );
    let stated: Vec<(&Value, Option<&Value>)> = (output["stated"].as_array().unwrap().iter())
        .map(|each| (&each["rule"], each.get("state")))
        .collect();
    let described = json!("described");
    assert_eq!(
        stated,
        [
            (&json!("/controls/judged"), None),
            (&json!("/controls/plain"), Some(&described)),
            (&json!("/rules/plain"), Some(&described)),
        ]
    );
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
    let controls: Vec<&Value> = (output["rules"].as_array().unwrap().iter())
        .map(|item| &item["rule"])
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
    assert_eq!(refused.code, "rules::rule::degraded");
    assert_eq!(
        refused.fields["rules"],
        json!([{"rule": "/controls/cites", "code": "board::ref::dangling"}])
    );
    let cited = citing("/controls/sources/paper");
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("controls/sources/paper.json", &note.to_string()),
        ("controls/cites.json", &cited),
    ]);
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(output["stated"][0]["rule"], "/controls/cites");
}

/// Six controls on `docs/a.md`: an obligation, a budget and a stated
/// control that leave `show` out, and a twin of each that sets it.
fn shown() -> (Scenario, Recording) {
    shown_with(&[])
}

/// The six controls of [`shown`], and `more` files.
fn shown_with(more: &[(&str, &str)]) -> (Scenario, Recording) {
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
    let records = [
        ("controls/a-obligation.json", obligation(None)),
        ("controls/b-obligation-edit.json", obligation(Some("edit"))),
        ("controls/c-budget.json", budget(None)),
        ("controls/d-budget-start.json", budget(Some("start"))),
        ("controls/e-stated.json", stated(None)),
        ("controls/f-stated-failing.json", stated(Some("failing"))),
    ];
    let mut files = vec![("docs/a.md", "one")];
    files.extend(records.iter().map(|(path, text)| (*path, text.as_str())));
    files.extend_from_slice(more);
    recording(&files)
}

/// Each item's control, kind and show.
fn kinds(items: &Value) -> Vec<(String, String, String)> {
    (items.as_array().unwrap().iter())
        .map(|item| {
            let field = |name: &str| item[name].as_str().unwrap().to_owned();
            (field("rule"), field("kind"), field("show"))
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
        kinds(&output["rules"]),
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

/// The six controls of [`shown`], a budget obligation that is the twin of
/// `c-budget` but for its kind, a standing control that is the twin of
/// `b-obligation-edit` but for its kind, and a version obligation and its
/// standing twin.
fn owed() -> (Scenario, Recording) {
    let owed = control(&json!({"paths": ["docs/*"], "rule": words(9), "kind": "obligation"}));
    let edit = control(&json!({"paths": ["docs/*"], "kind": "standing", "show": "edit"}));
    let pinned = |kind: &str| {
        let rule = json!({"check": "version", "at_least": "1.2", "capture": "v([0-9.]+)"});
        control(&json!({"paths": ["docs/*"], "rule": rule, "kind": kind}))
    };
    shown_with(&[
        ("controls/g-budget-owed.json", &owed),
        ("controls/h-standing-edit.json", &edit),
        ("controls/i-version-owed.json", &pinned("obligation")),
        ("controls/j-version-standing.json", &pinned("standing")),
    ])
}

/// Each item's control and its `close` line, null when it has none.
fn closes(items: &Value) -> Vec<(String, Value)> {
    (items.as_array().unwrap().iter())
        .map(|item| {
            let control = item["rule"].as_str().unwrap().to_owned();
            (control, item.get("close").cloned().unwrap_or(Value::Null))
        })
        .collect()
}

/// The `close` line an obligation at `control` carries.
fn close(control: &str) -> Value {
    json!(format!(
        "Once the work this obligation names is committed, delete it with `board rm \
         /controls/{control} -m '<message naming the work>'`; merging the work does not clear \
         it."
    ))
}

#[test]
fn check_for_path_and_for_paths_tell_how_to_close_an_obligation_and_give_a_standing_control_no_close()
 {
    let (_scenario, host) = owed();
    let output = check(&host, &json!({})).unwrap();
    let stated = [
        ("/controls/a-obligation".to_owned(), close("a-obligation")),
        (
            "/controls/b-obligation-edit".to_owned(),
            close("b-obligation-edit"),
        ),
        ("/controls/e-stated".to_owned(), Value::Null),
        ("/controls/f-stated-failing".to_owned(), Value::Null),
        ("/controls/h-standing-edit".to_owned(), Value::Null),
    ];
    assert_eq!(closes(&output["stated"]), stated);
    let budgets = [
        ("/controls/c-budget".to_owned(), Value::Null),
        ("/controls/d-budget-start".to_owned(), Value::Null),
        ("/controls/g-budget-owed".to_owned(), close("g-budget-owed")),
    ];
    assert_eq!(closes(&output["measured"]), budgets);
    let versions = [
        (
            "/controls/i-version-owed".to_owned(),
            close("i-version-owed"),
        ),
        ("/controls/j-version-standing".to_owned(), Value::Null),
    ];
    assert_eq!(closes(&output["versions"]), versions);
    let all = [
        stated[0].clone(),
        stated[1].clone(),
        budgets[0].clone(),
        budgets[1].clone(),
        stated[2].clone(),
        stated[3].clone(),
        budgets[2].clone(),
        stated[4].clone(),
        versions[0].clone(),
        versions[1].clone(),
    ];
    let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
    assert_eq!(closes(&output["rules"]), all);
    let output = for_paths(&host, &json!({"paths": ["/docs/a.md", "/docs/**"]})).unwrap();
    assert_eq!(closes(&output["rules"]), all);
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

/// A board whose copy of the control schema was edited, so it admits each
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

/// A control whose kind this Board does not know reads as standing, as its
/// standing twin does, in `check` and `for-path`.
#[test]
fn a_kind_this_board_does_not_know_reads_as_standing_like_its_standing_twin() {
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
            kinds(&output["rules"]),
            [row("a", "standing", "edit")],
            "{kind}"
        );
    }
}

/// A control whose show this Board does not know is shown at the start, the
/// first point, as its twin that says `start` is.
#[test]
fn a_show_this_board_does_not_know_reads_as_start_like_its_start_twin() {
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

/// A rule whose `check`, budget unit, `per` or pattern `quote` this Board
/// does not know is not applied: `check` states its control with its
/// requirement and measures nothing, and `evaluate` gives it `stated`. Its
/// twin with known values is measured and evaluated.
#[test]
fn a_rule_holding_a_value_this_board_does_not_know_is_stated_and_its_twin_is_applied() {
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
            .filter(|item| item["rule"] == "/controls/unknown")
            .collect();
        assert_eq!(stated.len(), 1, "{unknown}: {output}");
        assert_eq!(stated[0]["requirement"], "R.");
        let measured = |items: &Value, control: &str| {
            (items.as_array().unwrap().iter()).any(|item| item["rule"] == control)
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
            json!({"rules": ["/controls/known", "/controls/unknown"], "parts": {"body": "one"}});
        let output = crate::evaluate::evaluate(&host, &input).unwrap();
        let results = &output["results"];
        assert_eq!(
            (&results[0]["checker"], &results[0]["status"]),
            (&json!(verdict), &json!("passed")),
            "{known}"
        );
        assert_eq!(
            (&results[1]["checker"], &results[1]["status"]),
            (&Value::Null, &json!("stated")),
            "{unknown}"
        );
        let output = for_path(&host, &json!({"path": "/docs/a.md"})).unwrap();
        let rules: Vec<&Value> = (output["rules"].as_array().unwrap().iter())
            .map(|item| &item["checker"])
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

/// The suite a zero rule names in these tests.
const SUITE: &str = "/suites/old-path-users";

/// A `runs/suite/v1` record.
fn suite() -> String {
    json!({"$type": "runs/suite/v1", "method": "Count what still uses the old path."}).to_string()
}

/// A full run of a clean revision of [`SUITE`] that started at `started`
/// with `measures`, and `fields` added or replacing its own.
fn run(started: &str, measures: &Value, fields: &Value) -> String {
    let mut record = json!({
        "$type": "runs/run/v1",
        "suite": SUITE,
        "by": {"program": "claude-code"},
        "started": started,
        "scope": "full",
        "dirty": false,
        "results": [],
        "measures": measures,
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record.to_string()
}

/// A rule on `src/*` that `measures` of [`SUITE`] are 0, with `fields`
/// added to its checker.
fn zero_rule(measures: &[&str], fields: &Value) -> String {
    let mut checker = json!({"check": "zero", "suite": SUITE, "measures": measures});
    for (name, value) in fields.as_object().unwrap() {
        checker[name] = value.clone();
    }
    json!({
        "$type": "rules/rule/v1",
        "requirement": "Delete the old path once nothing uses it.",
        "purpose": "P.",
        "paths": ["src/*"],
        "checker": checker,
    })
    .to_string()
}

/// `check` under `/` of a board that enables rules and runs, holding the
/// old path, the rule `rule`, [`SUITE`] and `runs` by file name, at
/// `seconds` after 1970 began.
fn zero_check(rule: &str, runs: &[(&str, String)], seconds: u64) -> Value {
    let mut files = vec![
        ("src/old.rs", "fn old() {}".to_owned()),
        ("rules/old-path.json", rule.to_owned()),
        ("suites/old-path-users.json", suite()),
    ];
    for (name, text) in runs {
        files.push((name, text.clone()));
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    let (_scenario, mut host) = enabling(&["rules", "runs"], &files);
    host.settings.now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds);
    check(&host, &json!({})).unwrap()
}

/// The one zero entry of `output`.
fn the_zero(output: &Value) -> &Value {
    let [zero] = output["zeros"].as_array().unwrap().as_slice() else {
        panic!("one zero entry: {output}")
    };
    zero
}

const DAY: u64 = 86_400;

#[test]
fn a_zero_rule_passes_when_the_latest_run_reports_every_named_measure_as_0() {
    let rule = zero_rule(&["remaining", "unknown"], &json!({}));
    let measures = json!({"remaining": 0, "unknown": 0.0, "users": 12});
    let runs = [(
        "suites/old-path-users/runs/1.json",
        run("1970-01-01T00:00:00Z", &measures, &json!({})),
    )];
    let output = zero_check(&rule, &runs, 72_000);
    assert_eq!(output["passed"], true, "{output}");
    assert_eq!(
        the_zero(&output),
        &json!({
            "rule": "/rules/old-path",
            "requirement": "Delete the old path once nothing uses it.",
            "suite": SUITE,
            "run": "/suites/old-path-users/runs/1",
            "age_s": 72_000,
            "measures": [{"name": "remaining", "value": 0}, {"name": "unknown", "value": 0.0}],
            "state": "zero",
            "kind": "standing",
            "show": "failing",
        })
    );
    let stated: Vec<&Value> = (output["stated"].as_array().unwrap().iter())
        .map(|item| &item["rule"])
        .collect();
    assert!(stated.is_empty(), "a zero rule is measured: {output}");
}

#[test]
fn a_zero_rule_fails_when_a_named_measure_is_not_0_and_names_its_value() {
    let rule = zero_rule(&["remaining", "unknown"], &json!({}));
    let runs = [(
        "suites/old-path-users/runs/1.json",
        run(
            "1970-01-01T00:00:00Z",
            &json!({"remaining": 3, "unknown": 0}),
            &json!({}),
        ),
    )];
    let output = zero_check(&rule, &runs, 10);
    assert_eq!(output["passed"], false, "{output}");
    let zero = the_zero(&output);
    assert_eq!(zero["state"], "not_zero");
    assert_eq!(
        zero["measures"],
        json!([{"name": "remaining", "value": 3}, {"name": "unknown", "value": 0}])
    );
}

#[test]
fn a_zero_rule_reads_the_latest_qualifying_run_and_ignores_a_newer_dirty_one() {
    let rule = zero_rule(&["remaining"], &json!({}));
    let runs = [
        (
            "suites/old-path-users/runs/1.json",
            run("1970-01-01T00:00:00Z", &json!({"remaining": 0}), &json!({})),
        ),
        (
            "suites/old-path-users/runs/2.json",
            run(
                "1970-01-01T00:01:00Z",
                &json!({"remaining": 5}),
                &json!({"dirty": true}),
            ),
        ),
    ];
    let output = zero_check(&rule, &runs, 120);
    assert_eq!(output["passed"], true, "{output}");
    let zero = the_zero(&output);
    assert_eq!(
        (&zero["run"], &zero["state"]),
        (&json!("/suites/old-path-users/runs/1"), &json!("zero"))
    );
}

#[test]
fn a_zero_rule_is_unreadable_when_a_run_it_cannot_read_may_be_newer_than_a_clean_one() {
    let rule = zero_rule(&["remaining"], &json!({"within_days": 2}));
    let older = (
        "suites/old-path-users/runs/1.json",
        run("1970-01-01T00:00:00Z", &json!({"remaining": 0}), &json!({})),
    );
    let newer = |result: Value| {
        (
            "suites/old-path-users/runs/2.json",
            run(
                "1970-01-01T00:01:00Z",
                &json!({"remaining": 5}),
                &json!({"results": [result]}),
            ),
        )
    };
    let unread = zero_check(
        &rule,
        &[
            older.clone(),
            newer(json!({"case": "a", "outcome": "fail"})),
        ],
        120,
    );
    assert_eq!(unread["passed"], false, "{unread}");
    let zero = the_zero(&unread);
    assert_eq!(
        (&zero["state"], &zero["code"], &zero["run"]),
        (
            &json!("unreadable"),
            &json!("rules::zero::run_unreadable"),
            &Value::Null
        )
    );
    let read = zero_check(
        &rule,
        &[
            older,
            newer(json!({"case": "a", "outcome": "fail", "note": "n"})),
        ],
        120,
    );
    assert_eq!(read["passed"], false, "{read}");
    assert_eq!(
        (&the_zero(&read)["state"], &the_zero(&read)["run"]),
        (&json!("not_zero"), &json!("/suites/old-path-users/runs/2"))
    );
}

#[test]
fn a_zero_rule_fails_as_no_run_when_the_suite_has_no_qualifying_run() {
    let rule = zero_rule(&["remaining"], &json!({}));
    let runs = [(
        "suites/old-path-users/runs/1.json",
        run(
            "1970-01-01T00:00:00Z",
            &json!({"remaining": 0}),
            &json!({"scope": "targeted"}),
        ),
    )];
    let output = zero_check(&rule, &runs, 10);
    assert_eq!(output["passed"], false, "{output}");
    let zero = the_zero(&output);
    assert_eq!(zero["state"], "no_run");
    assert_eq!((&zero["run"], &zero["age_s"]), (&Value::Null, &Value::Null));
    assert_eq!(
        zero["measures"],
        json!([{"name": "remaining", "value": null}])
    );
}

#[test]
fn a_zero_rule_fails_as_missing_measure_when_the_run_lacks_a_named_measure() {
    let rule = zero_rule(&["remaining", "unknown"], &json!({}));
    for measures in [
        json!({"remaining": 0}),
        json!({"remaining": 0, "unknown": "none"}),
    ] {
        let runs = [(
            "suites/old-path-users/runs/1.json",
            run("1970-01-01T00:00:00Z", &measures, &json!({})),
        )];
        let output = zero_check(&rule, &runs, 10);
        assert_eq!(output["passed"], false, "{output}");
        let zero = the_zero(&output);
        assert_eq!(zero["state"], "missing_measure", "{measures}");
        assert_eq!(
            zero["measures"][0],
            json!({"name": "remaining", "value": 0})
        );
        assert_eq!(zero["measures"][1]["name"], "unknown");
    }
}

#[test]
fn a_zero_rule_fails_as_stale_when_the_run_is_older_than_within_days() {
    let rule = zero_rule(&["remaining"], &json!({"within_days": 2}));
    let runs = [(
        "suites/old-path-users/runs/1.json",
        run("1970-01-01T00:00:00Z", &json!({"remaining": 0}), &json!({})),
    )];
    let stale = zero_check(&rule, &runs, 2 * DAY + 1);
    assert_eq!(stale["passed"], false, "{stale}");
    assert_eq!(the_zero(&stale)["state"], "stale");
    let fresh = zero_check(&rule, &runs, 2 * DAY);
    assert_eq!(fresh["passed"], true, "{fresh}");
    assert_eq!(the_zero(&fresh)["state"], "zero");
}

#[test]
fn a_zero_rule_is_unreadable_and_check_still_answers_when_the_suite_is_missing() {
    let budget = json!({
        "$type": "rules/rule/v1",
        "requirement": "Short.",
        "purpose": "P.",
        "paths": ["src/*"],
        "checker": words(9),
    })
    .to_string();
    let missing = zero_rule(&["remaining"], &json!({})).replace(SUITE, "/suites/gone");
    let files = [
        ("src/old.rs", "fn old() {}"),
        ("rules/budget.json", budget.as_str()),
        ("rules/old-path.json", missing.as_str()),
    ];
    let (_scenario, host) = enabling(&["rules", "runs"], &files);
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(output["passed"], false, "{output}");
    assert_eq!(output["measured"][0]["used"], 3);
    let zero = the_zero(&output);
    assert_eq!(
        (&zero["state"], &zero["code"], &zero["suite"]),
        (
            &json!("unreadable"),
            &json!("runs::suite::not_found"),
            &json!("/suites/gone")
        )
    );
    assert_eq!(
        host.resolved(),
        [
            Call::GetEach(vec!["/src/old.rs".to_owned()]),
            Call::Called("runs.latest".to_owned()),
        ],
        "the zero rule reads no file and calls runs.latest once"
    );
}

#[test]
fn check_at_the_root_of_a_board_with_thousands_of_files_and_patterns_answers_within_the_query_limit()
 {
    // 4,000 files and 10,000 patterns: comparing each file with each
    // pattern makes 40 million GLOBs, past the 1-second query limit.
    let files: Vec<(String, String)> = (0..40)
        .flat_map(|dir| {
            (0..100).map(move |file| (format!("src/d{dir:02}/f{file:03}.txt"), "x".to_owned()))
        })
        .collect();
    // Rule r matches r + 1 files in one directory by `?`; its other
    // patterns share that directory's prefix and match nothing.
    let rule = |r: usize, extra: &[&str]| {
        let mut paths: Vec<String> = (0..200)
            .map(|j| {
                if j <= r {
                    format!("src/d{:02}/f{j:03}.t?t", r % 40)
                } else {
                    format!("src/d{:02}/none{j}*", j % 40)
                }
            })
            .collect();
        paths.extend(extra.iter().map(|path| (*path).to_owned()));
        json!({
            "$type": "rules/rule/v1",
            "requirement": "R.",
            "purpose": "P.",
            "paths": paths,
            "checker": words(100_000),
        })
        .to_string()
    };
    let rules: Vec<(String, String)> = (0..50)
        .map(|r| {
            let extra: &[&str] = match r {
                // A whole directory, one file of which a `?` pattern reaches too.
                0 => &["src/d39/*", "src/d00/f000.txt"],
                // No literal prefix, a range and a negated set.
                49 => &["*/d0[0-1]/f09[^0-8].txt"],
                _ => &[],
            };
            (format!("rules/r{r:02}.json"), rule(r, extra))
        })
        .collect();
    let all: Vec<(&str, &str)> = (files.iter().chain(&rules))
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let (_scenario, host) = enabling(&["rules"], &all);
    let output = check(&host, &json!({})).unwrap();
    let used: Vec<(&str, u64)> = (output["measured"].as_array().unwrap().iter())
        .map(|each| {
            (
                each["rule"].as_str().unwrap(),
                each["used"].as_u64().unwrap(),
            )
        })
        .collect();
    let expected: Vec<(String, u64)> = (0..50u64)
        .map(|r| {
            let count = match r {
                0 => 101,
                49 => 52,
                _ => r + 1,
            };
            (format!("/rules/r{r:02}"), count)
        })
        .collect();
    let expected: Vec<(&str, u64)> = (expected.iter())
        .map(|(rule, count)| (rule.as_str(), *count))
        .collect();
    assert_eq!(used, expected);
}

#[test]
fn a_rule_degraded_only_by_an_unreadable_reference_stays_live_and_fails_passed_not_the_call() {
    let library = Scenario::new();
    library.commit(&[("x.md", "one")]);
    let unpinned = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{}"}}"#,
        library.path(),
        "09".repeat(20)
    );
    let rule = |target: &str| {
        control(&json!({"paths": ["docs/*"], "applies_to": [target]}))
            .replace("controls/control/v1", "rules/rule/v1")
    };
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("vendor.json", &unpinned),
        ("rules/owed.json", &rule("/vendor/x")),
    ]);
    let output = check(&host, &json!({})).unwrap();
    assert_eq!(output["passed"], false, "{output}");
    assert_eq!(output["stated"][0]["rule"], "/rules/owed", "{output}");
    let warnings = output["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{output}");
    assert_eq!(warnings[0]["code"], "board::ref::unreadable");
    let detail = warnings[0]["detail"].as_str().unwrap();
    for named in ["/rules/owed", "/vendor/x", "again", "do not delete"] {
        assert!(detail.contains(named), "{named}: {detail}");
    }
    let (_scenario, host) = recording(&[
        ("docs/a.md", "one"),
        ("vendor.json", &unpinned),
        ("rules/owed.json", &rule("/nowhere")),
    ]);
    let refused = check(&host, &json!({})).unwrap_err();
    assert_eq!(
        refused.code, "rules::rule::degraded",
        "a dangling rule still fails"
    );
}

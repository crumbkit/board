//! Bindings fitted to their views by put and check alike, and pages read
//! for the app.

use board_model::{Code, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::{Value, json};

use crate::put::{Expect, PutReport};
use crate::tests::{board, open};

const VIEW: &str = r#"{"$type": "ui/view/v1", "slots": [
    {"name": "summary", "kind": "markdown"},
    {"name": "task", "kind": "record"}
]}"#;

fn scenario() -> Scenario {
    let scenario = board();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write("views/review.json", Item::File(VIEW))
        .write(
            "tasks/a.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "A"}"#),
        )
        .write(
            "summaries/a.json",
            Item::File(
                r#"{"$type": "ui/derived/v1", "from": ["/tasks/a"], "agent": "claude-code",
                    "model": "claude-opus-5-5", "markdown": "Small."}"#,
            ),
        );
    scenario
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn binding(summary: Value, task: Value) -> String {
    json!({
        "$type": "ui/binding/v1",
        "view": "/views/review",
        "slots": serde_json::Map::from_iter([
            ("summary".to_owned(), summary),
            ("task".to_owned(), task),
        ]),
    })
    .to_string()
}

fn put(scenario: &Scenario, record: &str) -> Result<(), (Code, Option<String>)> {
    open(scenario)
        .put(&path("/bindings/review"), Expect::Any, record.as_bytes())
        .map(drop)
        .map_err(|error| (error.code(), error.fields().pointer))
}

fn task() -> Value {
    json!({"ref": "/tasks/a"})
}

fn markdown() -> Value {
    json!({"data": {"markdown": "x"}})
}

#[test]
fn a_page_reads_each_slot_in_view_order_and_reports_a_slot_that_fails() {
    let scenario = scenario();
    put(&scenario, &binding(json!({"ref": "/summaries/a"}), task())).unwrap();
    scenario.write("tasks/a.json", Item::Deleted);

    let page = open(&scenario).page(&path("/bindings/review")).unwrap();
    assert_eq!(page.view, path("/views/review"));
    let [summary, task] = &page.slots[..] else {
        panic!("{page:?}")
    };
    assert_eq!(
        (summary.name.as_str(), summary.kind),
        ("summary", "markdown")
    );
    assert_eq!(summary.data.as_ref().unwrap()["markdown"], "Small.");
    assert_eq!(task.reference, Some(path("/tasks/a")));
    assert_eq!(task.data, None);
    assert_eq!(task.failed.as_ref().unwrap().code, "board::ref::dangling");
}

#[test]
fn a_page_for_a_record_that_is_not_a_binding_is_refused_and_one_for_a_binding_is_drawn() {
    let scenario = scenario();
    let board = open(&scenario);
    let refused = board.page(&path("/tasks/a")).unwrap_err();
    assert_eq!(refused.code(), Code::UiNotABinding);
    put(&scenario, &binding(markdown(), task())).unwrap();
    assert!(board.page(&path("/bindings/review")).is_ok());
}

#[test]
fn a_page_that_names_nothing_is_not_found() {
    let refused = open(&scenario()).page(&path("/bindings/none")).unwrap_err();
    assert_eq!(refused.code(), Code::PathNotFound);
}

#[test]
fn a_binding_outside_bindings_is_not_opened_and_the_same_binding_under_bindings_is() {
    let scenario = scenario();
    let full = binding(markdown(), task());
    scenario
        .write("pages/review.json", Item::File(&full))
        .write("bindings/review.json", Item::File(&full));
    let board = open(&scenario);
    let refused = board.binding(&path("/pages/review")).unwrap_err();
    assert_eq!(refused.code(), Code::UiNotABinding);
    assert!(board.binding(&path("/bindings/review")).is_ok());
}

const PATCH: &str = "--- a/src/retry.rs\n+++ b/src/retry.rs\n@@ -1,2 +1,2 @@\n-3\n+5\n }\n";

fn diff_binding(fill: &Value) -> String {
    json!({"$type": "ui/binding/v1", "view": "/views/change", "slots": {"change": fill}})
        .to_string()
}

fn diff_scenario() -> Scenario {
    let scenario = scenario();
    scenario.write(
        "views/change.json",
        Item::File(r#"{"$type": "ui/view/v1", "slots": [{"name": "change", "kind": "diff"}]}"#),
    );
    scenario
}

#[test]
fn a_page_draws_a_diff_slot_as_its_files_with_numbered_lines() {
    let scenario = diff_scenario();
    put(&scenario, &diff_binding(&json!({"data": {"patch": PATCH}}))).unwrap();
    let page = open(&scenario).page(&path("/bindings/review")).unwrap();
    assert_eq!(
        page.slots[0].data,
        Some(json!({"files": [{
            "old": "src/retry.rs", "new": "src/retry.rs", "added": 1, "removed": 1,
            "binary": false, "hunks": [{"header": "@@ -1,2 +1,2 @@", "lines": [
                {"change": "removed", "old": 1, "text": "3"},
                {"change": "added", "new": 1, "text": "5"},
                {"change": "context", "old": 2, "new": 2, "text": "}"},
            ]}],
        }]}))
    );
}

/// A code and the pointer it names, or `None` for a binding that fits.
type Judged = Option<(Code, Option<String>)>;

/// What check says of the binding at `/bindings/review`.
fn checked(scenario: &Scenario) -> Judged {
    let report = open(scenario).check(None, None).unwrap();
    let entry = report
        .entries
        .into_iter()
        .find(|entry| entry.path == "bindings/review.json")
        .unwrap();
    match entry.status {
        Status::Ok => None,
        Status::Degraded(code) => Some((code, entry.detail.and_then(|detail| detail.pointer))),
    }
}

/// What put says of `record` written at `/bindings/review`.
fn degraded(report: &PutReport) -> Judged {
    (report.writes[0].degraded.clone()).map(|(code, detail)| (code, detail.pointer))
}

/// What put, and then check, say of `record` put at `/bindings/review`.
fn judged(scenario: &Scenario, record: &str) -> (Judged, Judged) {
    let report = open(scenario)
        .put(&path("/bindings/review"), Expect::Any, record.as_bytes())
        .unwrap();
    (degraded(&report), checked(scenario))
}

fn at(code: Code, pointer: &str) -> (Code, Option<String>) {
    (code, Some(pointer.to_owned()))
}

#[test]
fn put_and_check_share_one_rule_and_give_a_binding_that_does_not_fit_the_same_code_and_pointer() {
    let full = binding(markdown(), task());
    let partial =
        json!({"$type": "ui/binding/v1", "view": "/views/review", "slots": {"task": task()}});
    let short = PATCH.replace(" }\n", "");
    let cases = [
        (
            binding(json!({"data": {"text": "x"}}), task()),
            Some(at(Code::UiSlotInvalid, "/slots/summary/data")),
        ),
        (
            binding(json!({"ref": "/tasks/a"}), task()),
            Some(at(Code::RefWrongType, "/slots/summary/ref")),
        ),
        (
            binding(markdown(), json!({"ref": "/tasks/b"})),
            Some(at(Code::RefDangling, "/slots/task/ref")),
        ),
        (
            partial.to_string(),
            Some(at(Code::UiSlotMismatch, "/slots")),
        ),
        (
            full.replace("/views/review", "/tasks/a"),
            Some(at(Code::RefWrongType, "/view")),
        ),
        (
            full.replace("/views/review", "/views/later"),
            Some(at(Code::RefDangling, "/view")),
        ),
        (
            diff_binding(&json!({"data": {"patch": short}})),
            Some(at(Code::UiSlotInvalid, "/slots/change/data/patch")),
        ),
        (
            diff_binding(&json!({"ref": "/tasks/a"})),
            Some(at(Code::RefWrongType, "/slots/change/ref")),
        ),
    ];
    let scenario = diff_scenario();
    for (record, expected) in cases {
        assert_eq!(
            judged(&scenario, &record),
            (expected.clone(), expected),
            "{record}"
        );
    }
}

#[test]
fn put_and_check_share_one_rule_and_admit_a_binding_that_fits_alike() {
    let scenario = diff_scenario();
    for record in [
        binding(markdown(), task()),
        binding(json!({"ref": "/summaries/a"}), task()),
        diff_binding(&json!({"data": {"patch": PATCH}})),
    ] {
        assert_eq!(judged(&scenario, &record), (None, None), "{record}");
    }
}

#[test]
fn check_degrades_a_binding_whose_reference_is_deleted_after_put_and_not_an_untouched_one() {
    let scenario = scenario();
    put(&scenario, &binding(markdown(), task())).unwrap();
    assert_eq!(checked(&scenario), None);
    scenario.write("tasks/a.json", Item::Deleted);
    assert_eq!(
        checked(&scenario),
        Some(at(Code::RefDangling, "/slots/task/ref"))
    );
    let report = open(&scenario).check(None, None).unwrap();
    let entry = (report.entries.iter())
        .find(|entry| entry.path == "bindings/review.json")
        .unwrap();
    let reason = &entry.detail.as_ref().unwrap().reason;
    assert!(
        reason.contains("/tasks/a, at /slots/task/ref, does not resolve: "),
        "{reason}"
    );
}

#[test]
fn a_check_with_a_warm_index_degrades_a_binding_whose_view_changed_and_admits_it_once_restored() {
    let scenario = scenario();
    put(&scenario, &binding(markdown(), task())).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let status = || {
        let report = open(&scenario).check(None, Some(cache.path())).unwrap();
        let entry = (report.entries.into_iter())
            .find(|entry| entry.path == "bindings/review.json")
            .unwrap();
        entry.status
    };
    assert_eq!(status(), Status::Ok, "cold");
    let grown = VIEW.replace(
        r#"{"name": "task", "kind": "record"}"#,
        r#"{"name": "task", "kind": "record"}, {"name": "notes", "kind": "markdown"}"#,
    );
    scenario.write("views/review.json", Item::File(&grown));
    assert_eq!(status(), Status::Degraded(Code::UiSlotMismatch), "changed");
    scenario.write("views/review.json", Item::File(VIEW));
    assert_eq!(status(), Status::Ok, "restored");
}

#[test]
fn check_degrades_a_binding_whose_view_gains_a_slot_after_put_and_not_an_untouched_one() {
    let scenario = scenario();
    put(&scenario, &binding(markdown(), task())).unwrap();
    assert_eq!(checked(&scenario), None);
    let grown = VIEW.replace(
        r#"{"name": "task", "kind": "record"}"#,
        r#"{"name": "task", "kind": "record"}, {"name": "notes", "kind": "markdown"}"#,
    );
    scenario.write("views/review.json", Item::File(&grown));
    assert_eq!(checked(&scenario), Some(at(Code::UiSlotMismatch, "/slots")));
}

fn line(path: &str, record: &str) -> String {
    format!(r#"{{"path": "{path}", "record": {record}}}"#) + "\n"
}

#[test]
fn a_binding_and_its_view_put_in_one_batch_fit_in_either_order_and_a_binding_without_its_view_is_degraded()
 {
    let view = line(
        "/views/later",
        &serde_json::from_str::<Value>(VIEW).unwrap().to_string(),
    );
    let bound = line(
        "/bindings/review",
        &binding(markdown(), task()).replace("/views/review", "/views/later"),
    );
    for input in [view.clone() + &bound, bound.clone() + &view] {
        let scenario = scenario();
        let report = open(&scenario).put_batch(&mut input.as_bytes()).unwrap();
        let fits: Vec<_> = (report.writes.iter())
            .map(|written| &written.degraded)
            .collect();
        assert_eq!(fits, [&None, &None], "{input}");
        assert_eq!(checked(&scenario), None, "{input}");
    }
    let scenario = scenario();
    let report = open(&scenario).put_batch(&mut bound.as_bytes()).unwrap();
    assert_eq!(degraded(&report), Some(at(Code::RefDangling, "/view")));
}

/// How many files a warm check of `scenario` opens: the second of two.
fn warm(scenario: &Scenario) -> usize {
    let cache = tempfile::tempdir().unwrap();
    let board = open(scenario);
    board.check(None, Some(cache.path())).unwrap();
    let before = board.repository().files_opened();
    board.check(None, Some(cache.path())).unwrap();
    board.repository().files_opened() - before
}

#[test]
fn a_warm_check_reads_a_binding_its_view_and_its_references_and_nothing_without_a_binding() {
    let scenario = scenario();
    assert_eq!(warm(&scenario), 0, "no binding");
    let fitted = binding(json!({"ref": "/summaries/a"}), task());
    scenario.write("bindings/review.json", Item::File(&fitted));
    // The binding, its view, the summary and the task, each opened once to
    // read it. Resolving their paths opens none, because their facts say
    // none holds a pointer.
    assert_eq!(warm(&scenario), 4, "one binding");
}

//! Bindings fitted to their views on put, and pages read for the app.

use board_model::{Code, LogicalPath};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::put::Expect;
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
fn slot_data_its_kind_refuses_is_refused_and_data_it_accepts_is_admitted() {
    let scenario = scenario();
    assert_eq!(
        put(&scenario, &binding(json!({"data": {"text": "x"}}), task())),
        Err((Code::UiSlotInvalid, Some("/slots/summary/data".to_owned())))
    );
    assert_eq!(put(&scenario, &binding(markdown(), task())), Ok(()));
}

#[test]
fn a_reference_to_a_type_the_slot_does_not_draw_is_refused_and_one_it_draws_is_admitted() {
    let scenario = scenario();
    assert_eq!(
        put(&scenario, &binding(json!({"ref": "/tasks/a"}), task())),
        Err((Code::RefWrongType, Some("/slots/summary/ref".to_owned())))
    );
    assert_eq!(
        put(&scenario, &binding(json!({"ref": "/summaries/a"}), task())),
        Ok(())
    );
}

#[test]
fn a_reference_that_does_not_resolve_is_refused_and_one_that_does_is_admitted() {
    let scenario = scenario();
    assert_eq!(
        put(&scenario, &binding(markdown(), json!({"ref": "/tasks/b"}))),
        Err((Code::RefDangling, Some("/slots/task/ref".to_owned())))
    );
    assert_eq!(put(&scenario, &binding(markdown(), task())), Ok(()));
}

#[test]
fn a_binding_missing_a_slot_its_view_names_is_refused_and_a_full_one_is_admitted() {
    let scenario = scenario();
    let partial = json!({"$type": "ui/binding/v1", "view": "/views/review",
        "slots": {"task": task()}})
    .to_string();
    assert_eq!(
        put(&scenario, &partial),
        Err((Code::UiSlotMismatch, Some("/slots".to_owned())))
    );
    assert_eq!(put(&scenario, &binding(markdown(), task())), Ok(()));
}

#[test]
fn a_binding_whose_view_is_not_a_view_is_refused_and_one_naming_a_view_is_admitted() {
    let scenario = scenario();
    let full = binding(markdown(), task());
    let wrong = full.replace("/views/review", "/tasks/a");
    assert_eq!(
        put(&scenario, &wrong),
        Err((Code::RefWrongType, Some("/view".to_owned())))
    );
    assert_eq!(put(&scenario, &full), Ok(()));
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
fn a_binding_whose_view_does_not_resolve_says_to_put_the_view_first_and_one_whose_view_resolves_is_admitted()
 {
    let scenario = scenario();
    let full = binding(markdown(), task());
    let refused = open(&scenario)
        .put(
            &path("/bindings/review"),
            Expect::Any,
            full.replace("/views/review", "/views/later").as_bytes(),
        )
        .unwrap_err();
    assert_eq!(
        (refused.code(), refused.fields().pointer.as_deref()),
        (Code::RefDangling, Some("/view"))
    );
    assert!(
        refused
            .to_string()
            .contains("put the view in an earlier put")
    );
    assert_eq!(put(&scenario, &full), Ok(()));
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
fn a_diff_slot_whose_patch_does_not_parse_is_refused_and_one_that_parses_is_admitted() {
    let scenario = diff_scenario();
    let short = PATCH.replace(" }\n", "");
    assert_eq!(
        put(&scenario, &diff_binding(&json!({"data": {"patch": short}}))),
        Err((
            Code::UiSlotInvalid,
            Some("/slots/change/data/patch".to_owned())
        ))
    );
    assert_eq!(
        put(&scenario, &diff_binding(&json!({"data": {"patch": PATCH}}))),
        Ok(())
    );
}

#[test]
fn a_diff_slot_given_by_reference_is_refused_and_the_same_slot_given_as_data_is_admitted() {
    let scenario = diff_scenario();
    assert_eq!(
        put(&scenario, &diff_binding(&json!({"ref": "/tasks/a"}))),
        Err((Code::RefWrongType, Some("/slots/change/ref".to_owned())))
    );
    assert_eq!(
        put(&scenario, &diff_binding(&json!({"data": {"patch": PATCH}}))),
        Ok(())
    );
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

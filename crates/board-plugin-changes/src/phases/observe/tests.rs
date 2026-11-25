//! What a plan-phase gate reading asks the rules plugin: one
//! `rules.for-paths` call, whose answer gives both the obligations and the
//! rules on the planned paths.

use serde_json::{Value, json};

use crate::flow::Flow;
use crate::records::ChangeRecord;
use crate::rules::tests::{PLAN, Ruled};

use super::observe_in;

const CHANGE: &str = "/changes/c";
const FOR_PATHS: &str = "rules.for-paths";

/// A flow whose plan phase holds `checks`.
fn flow(checks: &Value) -> Flow {
    serde_json::from_value(json!({
        "$type": "changes/flow/v1",
        "name": "test",
        "summary": "A flow for the test.",
        "stop": {"through": "plan"},
        "phases": [{"name": "plan", "agent": "lead", "instruction": "Plan.", "checks": checks}],
    }))
    .unwrap()
}

fn obligations_check() -> Value {
    json!({"id": "obligations", "kind": "obligations", "blocks": false})
}

fn planned_check() -> Value {
    json!({"id": "lifecycle-declared", "kind": "planned-rule-paths", "rules": ["/rules/lifecycle-paths"]})
}

/// A change in its plan phase whose plan is [`PLAN`].
fn change() -> ChangeRecord {
    let mut change = ChangeRecord::new("A change.".to_owned(), "/repos/board".to_owned(), "plan");
    change.plan = Some(PLAN.to_owned());
    change
}

/// One obligation, stored under the former prefix, and one other rule.
fn answer() -> Value {
    json!({"rules": [
        {"rule": "/rules/owed", "summary": "Owe it.", "close": "Close it."},
        {"rule": "/controls/style", "summary": "Keep the style."},
    ]})
}

#[test]
fn a_plan_phase_reading_asks_rules_for_paths_once_for_its_obligations_and_planned_rules() {
    let host = Ruled::new(Ok(answer()), &["/rules/owed"]);
    let checks = json!([obligations_check(), planned_check()]);
    let found = observe_in(&host, CHANGE, &change(), Some(flow(&checks))).unwrap();
    assert_eq!(host.calls_to(FOR_PATHS), 1);
    assert_eq!(found.obligations, ["/rules/owed"]);
    assert_eq!(found.on_planned, ["/rules/owed", "/rules/style"]);
    assert_eq!(found.on_planned_unread, None);
}

#[test]
fn a_refused_rules_for_paths_call_fails_a_reading_that_wants_obligations() {
    let checks = json!([obligations_check(), planned_check()]);
    let refusing = Ruled::new(Err("rules::index::broken"), &[]);
    let refused = observe_in(&refusing, CHANGE, &change(), Some(flow(&checks))).unwrap_err();
    assert_eq!(refused.code, "changes::board::refused");
    assert!(
        refused.message.contains("rules::index::broken"),
        "{}",
        refused.message
    );
    assert_eq!(refusing.calls_to(FOR_PATHS), 1);
    let answering = Ruled::new(Ok(answer()), &[]);
    assert!(observe_in(&answering, CHANGE, &change(), Some(flow(&checks))).is_ok());
}

#[test]
fn a_reading_that_wants_only_planned_rules_records_the_refusal_and_reads_on() {
    let checks = json!([planned_check()]);
    let refusing = Ruled::new(Err("rules::index::broken"), &[]);
    let found = observe_in(&refusing, CHANGE, &change(), Some(flow(&checks))).unwrap();
    let unread = found.on_planned_unread.unwrap();
    assert!(unread.contains("rules::index::broken"), "{unread}");
    assert_eq!(found.on_planned, Vec::<String>::new());
    assert_eq!(refusing.calls_to(FOR_PATHS), 1);
    let answering = Ruled::new(Ok(answer()), &[]);
    let found = observe_in(&answering, CHANGE, &change(), Some(flow(&checks))).unwrap();
    assert_eq!(found.on_planned, ["/rules/owed", "/rules/style"]);
    assert_eq!(found.on_planned_unread, None);
}

#[test]
fn a_reading_whose_plan_names_no_paths_asks_rules_nothing() {
    let host = Ruled::new(Ok(answer()), &["/rules/owed"]);
    let checks = json!([obligations_check(), planned_check()]);
    let mut planless = change();
    planless.plan = None;
    let found = observe_in(&host, CHANGE, &planless, Some(flow(&checks))).unwrap();
    assert_eq!(host.calls_to(FOR_PATHS), 0);
    assert_eq!(found.obligations, Vec::<String>::new());
    assert_eq!(found.on_planned, Vec::<String>::new());
}

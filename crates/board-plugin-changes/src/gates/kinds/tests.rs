//! The interpreter evaluates a flow's checks by kind: `when` decides which
//! apply, `field` and `records` read their parameters, and a kind Board
//! does not evaluate or know stays missing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::change::Referenced;
use crate::flow;
use crate::gates::{Observed, Status, interpret};
use crate::records::{ChangeRecord, Kind};

const PATH: &str = "/changes/t";
const HEAD: &str = "a3f9c0de1234567890abcdef1234567890abcdef";

/// A change in plan with `fields`, these attached records, and a plan whose
/// UX says No UI change.
fn observed(fields: &Value, records: &[(&str, &str, Value)]) -> Observed {
    let mut change = json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": "plan",
        "plan": "/changes/t/plans/1",
    });
    for (key, value) in fields.as_object().unwrap() {
        change[key] = value.clone();
    }
    let change: ChangeRecord = serde_json::from_value(change).unwrap();
    let records = Referenced {
        records: (records.iter())
            .map(|(path, kind, body)| {
                let body = body.as_object().unwrap().clone();
                ((*path).to_owned(), ((*kind).to_owned(), body))
            })
            .collect(),
        unreadable: Vec::new(),
        unreadable_types: BTreeMap::new(),
    };
    let mut found = Observed::new(PATH, change, Kind::Build, records);
    found.plan = Some("# T\n\n## UX\n\nNo UI change: it is headless.\n".to_owned());
    found.plan_file = Some("/changes/t/plans/1.md".to_owned());
    found
}

/// The checks of `checks`, a JSON list, as a flow holds them.
fn checks(checks: Value) -> &'static [flow::Check] {
    let parsed: Vec<flow::Check> = serde_json::from_value(checks).unwrap();
    Vec::leak(parsed)
}

/// Each applying check's id, status and detail.
fn run(observed: &Observed, list: Value) -> Vec<(&'static str, Status, String)> {
    (interpret(observed, checks(list)).into_iter())
        .map(|found| (found.check, found.status, found.detail))
        .collect()
}

#[test]
fn a_check_whose_route_or_section_condition_fails_is_left_out_and_its_twin_applies() {
    let list = json!([
        {"id": "asked", "kind": "field", "field": "head", "when": {"route": ["ask-first"]}},
        {"id": "ui", "kind": "field", "field": "head", "when": {"section": {"UX": "^no ui change\\b"}}},
        {"id": "not-ui", "kind": "field", "field": "head", "when": {"section_not": {"UX": "^no ui change\\b"}}},
    ]);
    let ids = |fields: Value| -> Vec<&'static str> {
        (run(&observed(&fields, &[]), list.clone()).into_iter())
            .map(|(id, _, _)| id)
            .collect()
    };
    assert_eq!(ids(json!({"route": "owner"})), ["ui"]);
    assert_eq!(ids(json!({"route": "ask-first"})), ["asked", "ui"]);
}

#[test]
fn a_pattern_that_does_not_compile_makes_its_check_missing_and_names_the_pattern() {
    let bad = json!([{"id": "x", "kind": "tasks", "when": {"section": {"UX": "(no"}}}]);
    let (_, status, detail) = run(&observed(&json!({}), &[]), bad).remove(0);
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with("the flow's pattern (no for the UX section is not a regular expression"),
        "{detail}"
    );
    let good = json!([{"id": "x", "kind": "tasks", "when": {"section": {"UX": "no"}}}]);
    assert_eq!(run(&observed(&json!({}), &[]), good)[0].1, Status::Pass);
}

#[test]
fn a_kind_board_does_not_evaluate_or_know_stays_missing_and_a_known_one_passes() {
    let list = json!([
        {"id": "c", "kind": "considered"},
        {"id": "s", "kind": "scripted"},
        {"id": "t", "kind": "tasks"},
    ]);
    let found = run(&observed(&json!({}), &[]), list);
    assert_eq!(
        found[0].2,
        "this Board does not evaluate considered checks yet, so the flow's check c cannot pass; \
         ask the owner whether to take it out of the flow"
    );
    assert_eq!(
        found[1].2,
        "the flow's check s has the kind scripted, which this Board does not know, so it cannot \
         pass; update Board or change the check in the flow"
    );
    let statuses: Vec<Status> = found.iter().map(|(_, status, _)| *status).collect();
    assert_eq!(statuses, [Status::Missing, Status::Missing, Status::Pass]);
}

#[test]
fn a_check_missing_a_parameter_its_kind_needs_is_missing_and_one_with_it_passes() {
    let fields = json!({"head": HEAD});
    let unset = run(
        &observed(&fields, &[]),
        json!([{"id": "h", "kind": "field"}]),
    );
    assert_eq!(
        unset[0].2,
        "the flow's check h names no field, so it cannot pass; add field to the check in the flow"
    );
    let set = run(
        &observed(&fields, &[]),
        json!([{"id": "h", "kind": "field", "field": "head"}]),
    );
    assert_eq!(set[0].1, Status::Pass);
}

/// A report on the head that approves, by `person`.
fn approval(person: &str) -> Value {
    json!({"range": format!("b..{HEAD}"), "verdict": "approve", "findings": [], "by": {"person": person}})
}

#[test]
fn records_with_min_and_distinct_need_that_many_different_actors() {
    let list = json!([{
        "id": "two", "kind": "records", "type": "review/report/v1", "min": 2, "distinct": "by.person",
        "where": {"at_head": "range", "values": {"verdict": ["approve"]}}
    }]);
    let fields = json!({"head": HEAD});
    let same = [
        ("/r/1", "review/report/v1", approval("ann")),
        ("/r/2", "review/report/v1", approval("ann")),
    ];
    let found = run(&observed(&fields, &same), list.clone());
    assert_eq!(found[0].1, Status::Missing);
    assert_eq!(
        found[0].2,
        format!(
            "fewer than 2 attached review/report/v1 records from different persons cover {HEAD} \
             with a verdict of approve"
        )
    );
    let different = [
        ("/r/1", "review/report/v1", approval("ann")),
        ("/r/2", "review/report/v1", approval("bo")),
    ];
    assert_eq!(run(&observed(&fields, &different), list)[0].1, Status::Pass);
}

#[test]
fn records_whose_refs_must_name_an_attached_type_and_be_owner_signed() {
    let list = json!([{
        "id": "adopted", "kind": "records", "type": "plans/decision/v1",
        "where": {"owner_signed": true, "refs": ["evals/eval/v1"]}
    }]);
    let eval = ("/evals/e", "evals/eval/v1", json!({}));
    let stub = (
        "/d",
        "plans/decision/v1",
        json!({"refs": ["/evals/e"], "written_by": "hook"}),
    );
    let found = run(&observed(&json!({}), &[eval.clone(), stub]), list.clone());
    assert_eq!(
        found[0].2,
        "no plans/decision/v1 that the owner signed whose refs name an attached evals/eval/v1 is \
         attached"
    );
    let signed = ("/d", "plans/decision/v1", json!({"refs": ["/evals/e"]}));
    assert_eq!(
        run(&observed(&json!({}), &[eval, signed]), list)[0].1,
        Status::Pass
    );
}

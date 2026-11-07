//! The interpreter evaluates a flow's checks by kind: `when` decides which
//! apply, `field` and `records` read their parameters, and a kind Board
//! does not evaluate or know stays missing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::change::Referenced;
use crate::flow;
use crate::gates::{Observed, Status, interpret};
use crate::records::Kind;

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
    let change = crate::change::from_value(change).unwrap();
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
fn run(observed: &Observed, list: Value) -> Vec<(String, Status, String)> {
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
    let ids = |fields: Value| -> Vec<String> {
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
fn a_kind_board_does_not_know_stays_missing_and_a_known_one_passes() {
    let list = json!([
        {"id": "c", "kind": "flow-valid"},
        {"id": "s", "kind": "scripted"},
        {"id": "t", "kind": "tasks"},
    ]);
    let found = run(&observed(&json!({}), &[]), list);
    assert_eq!(found[0].2, "no changes/flow/v1 draft is attached");
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

/// The first check `list` gives for `observed`, whole.
fn first(observed: &Observed, list: Value) -> crate::gates::Check {
    interpret(observed, checks(list)).remove(0)
}

#[test]
fn a_surfaced_record_neither_used_nor_considered_is_missing_and_one_listed_under_considered_passes()
{
    let list = json!([{"id": "considered", "kind": "considered"}]);
    let mut found = observed(&json!({}), &[]);
    found.change.surfaced = vec!["/tasks/a".to_owned(), "/plans/d1".to_owned()];
    found.uses = vec!["/tasks/a".to_owned()];
    let check = first(&found, list.clone());
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "the plan neither uses nor lists under ### Considered these records that start surfaced: \
         /plans/d1"
    );
    found.plan = Some(
        "# T\n\n## UX\n\nNo UI change: it is headless.\n\n### Considered\n\n- /plans/d1: it is \
         about the old flow.\n"
            .to_owned(),
    );
    assert_eq!(first(&found, list).status, Status::Pass);
}

#[test]
fn an_obligation_on_the_plans_paths_is_missing_and_one_named_under_the_escape_heading_passes() {
    let list = json!([{"id": "settled", "kind": "obligations", "escape_section": "Obligations left owed"}]);
    let mut found = observed(&json!({}), &[]);
    found.obligations = vec!["/controls/x/docs-owed".to_owned()];
    let check = first(&found, list.clone());
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "these obligations apply to the plan's paths: /controls/x/docs-owed"
    );
    assert_eq!(
        check.remedy.unwrap(),
        "Do the work each obligation names. Once the work is committed, delete each obligation \
         with `board rm <path>`, which commits the deletion; merging the work does not clear \
         it. List an obligation the change leaves owed under the plan's escape heading."
    );
    found.plan = Some(
        "# T\n\n## Obligations left owed\n\n- /controls/x/docs-owed, owed to the next step.\n"
            .to_owned(),
    );
    assert_eq!(first(&found, list).status, Status::Pass);
}

#[test]
fn an_obligations_detail_names_ten_obligations_and_counts_the_rest() {
    let list = json!([{"id": "o", "kind": "obligations"}]);
    let mut found = observed(&json!({}), &[]);
    let owed = |count: usize| -> Vec<String> {
        (0..count)
            .map(|at| format!("/controls/x/o{at:02}"))
            .collect()
    };
    found.obligations = owed(10);
    let ten = first(&found, list.clone()).detail;
    assert!(ten.ends_with("/controls/x/o09"), "{ten}");
    found.obligations = owed(12);
    let twelve = first(&found, list).detail;
    assert!(twelve.ends_with("/controls/x/o09, and 2 more"), "{twelve}");
}

#[test]
fn a_missing_check_with_blocks_false_never_blocks_and_its_blocking_twin_does() {
    let mut found = observed(&json!({}), &[]);
    found.obligations = vec!["/controls/x/docs-owed".to_owned()];
    let quiet = first(
        &found,
        json!([{"id": "o", "kind": "obligations", "blocks": false}]),
    );
    assert_eq!(
        (quiet.status, quiet.blocks, quiet.blocking()),
        (Status::Missing, false, false)
    );
    let loud = first(&found, json!([{"id": "o", "kind": "obligations"}]));
    assert_eq!(
        (loud.status, loud.blocks, loud.blocking()),
        (Status::Missing, true, true)
    );
}

#[test]
fn a_check_reports_its_actor_and_an_agent_one_by_default() {
    let found = observed(&json!({}), &[]);
    let owned = first(
        &found,
        json!([{"id": "t", "kind": "tasks", "actor": "owner"}]),
    );
    assert_eq!(owned.actor, flow::Actor::Owner);
    let plain = first(&found, json!([{"id": "t", "kind": "tasks"}]));
    assert_eq!(plain.actor, flow::Actor::Agent);
}

#[test]
fn controlled_paths_pass_when_no_named_control_exists_and_say_so_and_miss_an_edit_a_control_lists()
{
    let list = json!([{"id": "ui-paths", "kind": "controlled-paths", "controls": ["/controls/{repo}/ui-paths"]}]);
    let mut found = observed(&json!({}), &[]);
    found.edited = vec!["/repos/x/ui/a.rs".to_owned()];
    let none = first(&found, list.clone());
    assert_eq!(none.status, Status::Pass);
    assert_eq!(
        none.detail,
        "no control /controls/x/ui-paths exists, so the board names no such paths and the check \
         passes"
    );
    found.controlled.insert(
        "/controls/x/ui-paths".to_owned(),
        Some(vec!["/repos/x/ui/a.rs".to_owned()]),
    );
    let touched = first(&found, list.clone());
    assert_eq!(touched.status, Status::Missing);
    assert_eq!(
        touched.detail,
        "the change's diff edits files that /controls/x/ui-paths list: /repos/x/ui/a.rs"
    );
    found
        .controlled
        .insert("/controls/x/ui-paths".to_owned(), Some(Vec::new()));
    let clear = first(&found, list);
    assert_eq!((clear.status, clear.detail), (Status::Pass, String::new()));
}

#[test]
fn a_controlled_paths_detail_names_ten_touched_files_and_counts_the_rest() {
    let list =
        json!([{"id": "ui-paths", "kind": "controlled-paths", "controls": ["/controls/x/ui"]}]);
    let mut found = observed(&json!({}), &[]);
    let files = |count: usize| -> Vec<String> {
        (0..count)
            .map(|at| format!("/repos/x/ui/{at:02}.rs"))
            .collect()
    };
    found
        .controlled
        .insert("/controls/x/ui".to_owned(), Some(files(10)));
    let ten = first(&found, list.clone()).detail;
    assert!(ten.ends_with("/repos/x/ui/09.rs"), "{ten}");
    found
        .controlled
        .insert("/controls/x/ui".to_owned(), Some(files(13)));
    let thirteen = first(&found, list).detail;
    assert!(
        thirteen.ends_with("/repos/x/ui/09.rs, and 3 more"),
        "{thirteen}"
    );
    assert!(!thirteen.contains("/repos/x/ui/10.rs"), "{thirteen}");
}

#[test]
fn a_tickets_check_with_types_needs_a_ticket_of_one_of_them() {
    let list =
        json!([{"id": "ticket", "kind": "field", "field": "tickets", "types": ["jira/issue/v1"]}]);
    let mut found = observed(&json!({"tickets": ["/jira/T-1"]}), &[]);
    found.ticket_types = vec!["github/issue/v1".to_owned()];
    let check = first(&found, list.clone());
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "the change references 0 tickets of type jira/issue/v1, and the flow's check ticket \
         needs 1"
    );
    found.ticket_types = vec!["jira/issue/v1".to_owned()];
    assert_eq!(first(&found, list).status, Status::Pass);
}

#[test]
fn a_waivable_check_passes_on_a_decision_the_owner_signed_naming_it_and_not_on_a_hooks_stub() {
    let list = json!([{"id": "suite-run", "kind": "run", "scope": "full", "waivable": true}]);
    let stub = (
        "/d/w",
        "plans/decision/v1",
        json!({"refs": ["/changes/t#suite-run"], "written_by": "hook"}),
    );
    let missing = first(&observed(&json!({"head": HEAD}), &[stub]), list.clone());
    assert_eq!(missing.status, Status::Missing);
    assert!(
        missing
            .remedy
            .unwrap()
            .contains("whose refs name /changes/t#suite-run"),
        "the remedy says how to waive the check"
    );
    let signed = (
        "/d/w",
        "plans/decision/v1",
        json!({"refs": ["/changes/t#suite-run"]}),
    );
    let waived = first(&observed(&json!({"head": HEAD}), &[signed]), list);
    assert_eq!(
        (waived.status, waived.detail),
        (Status::Pass, "the owner waived it in /d/w".to_owned())
    );
}

#[test]
fn a_check_that_is_not_waivable_ignores_a_decision_naming_it() {
    let list = json!([{"id": "suite-run", "kind": "run", "scope": "full"}]);
    let signed = (
        "/d/w",
        "plans/decision/v1",
        json!({"refs": ["/changes/t#suite-run"]}),
    );
    let found = first(&observed(&json!({"head": HEAD}), &[signed]), list);
    assert_eq!(found.status, Status::Missing);
}

//! The interpreter evaluates a flow's checks by kind: `when` decides which
//! apply, `field` and `records` read their parameters, and a kind Board
//! does not evaluate or know stays missing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::change::Referenced;
use crate::flow;
use crate::gates::tests::{PERSON, by, judged};
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
    let found = |fields: &Value| {
        let decided = (
            "/d",
            "plans/decision/v1",
            by(json!({"refs": ["/evals/e"]}), fields),
        );
        run(
            &observed(&json!({}), &[eval.clone(), decided]),
            list.clone(),
        )
        .remove(0)
    };
    let [got, wanted] = judged(|fields| found(fields).1 == Status::Pass);
    assert_eq!(got, wanted);
    assert_eq!(
        found(&json!({"by": {"program": "claude-code"}})).2,
        "no plans/decision/v1 that the owner signed with by.person whose refs name an attached \
         evals/eval/v1 is attached"
    );
}

#[test]
fn an_owners_answer_counts_only_where_refs_name_the_plan_or_the_change_and_no_head_is_named() {
    let options = json!([{"id": "a", "text": format!("Approve {PATH}.")}]);
    let records = [
        ("/q", "plans/question/v1", json!({"options": options})),
        (
            "/a",
            "plans/answer/v1",
            json!({"question": "/q", "choice": "a", "by": {"person": PERSON}}),
        ),
    ];
    let found = |rules: Value| {
        let list = json!([{"id": "c", "kind": "records", "type": "plans/decision/v1",
                           "where": rules}]);
        let (_, status, detail) = run(&observed(&json!({"head": HEAD}), &records), list).remove(0);
        (status, detail)
    };
    let refused = [
        (
            json!({"owner_signed": true, "refs": ["evals/eval/v1"]}),
            "no plans/decision/v1 that the owner signed with by.person whose refs name an \
             attached evals/eval/v1 is attached"
                .to_owned(),
        ),
        (
            json!({"owner_signed": true, "refs": []}),
            "no plans/decision/v1 that the owner signed with by.person is attached".to_owned(),
        ),
        (
            json!({"owner_signed": true, "refs": ["plan", "change"], "at_head": "range"}),
            format!(
                "no attached plans/decision/v1 that the owner signed with by.person covers \
                 {HEAD} whose refs name the plan or the change and the change"
            ),
        ),
    ];
    for (rules, detail) in refused {
        assert_eq!(found(rules.clone()), (Status::Missing, detail), "{rules}");
    }
    let admitted = json!({"owner_signed": true, "refs": ["plan", "change"]});
    assert_eq!(found(admitted).0, Status::Pass);
}

/// The first check `list` gives for `observed`, whole.
fn first(observed: &Observed, list: Value) -> crate::gates::Check {
    interpret(observed, checks(list)).remove(0)
}

#[test]
fn a_surfaced_record_neither_used_nor_considered_is_missing_and_one_in_considered_passes() {
    let list = json!([{"id": "considered", "kind": "considered"}]);
    let mut found = observed(&json!({}), &[]);
    found.change.surfaced = vec!["/tasks/a".to_owned(), "/plans/d1".to_owned()];
    found.uses = vec!["/tasks/a".to_owned()];
    let check = first(&found, list.clone());
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "the plan neither uses nor lists in considered these records that start surfaced: \
         /plans/d1"
    );
    found.considered = vec!["/plans/d1".to_owned()];
    assert_eq!(first(&found, list).status, Status::Pass);
}

#[test]
fn a_surfaced_record_that_no_longer_resolves_passes_and_one_that_exists_unconsidered_is_missing() {
    let list = json!([{"id": "considered", "kind": "considered"}]);
    let mut found = observed(&json!({}), &[]);
    found.change.surfaced = vec!["/plans/gone".to_owned()];
    found.surfaced_gone = vec!["/plans/gone".to_owned()];
    assert_eq!(first(&found, list.clone()).status, Status::Pass);
    found.change.surfaced.push("/plans/there".to_owned());
    let check = first(&found, list);
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "the plan neither uses nor lists in considered these records that start surfaced: \
         /plans/there"
    );
}

#[test]
fn a_surfaced_record_listed_only_under_a_considered_heading_in_the_text_is_missing() {
    let list = json!([{"id": "considered", "kind": "considered"}]);
    let mut found = observed(&json!({}), &[]);
    found.change.surfaced = vec!["/plans/d1".to_owned()];
    found.plan = Some(
        "# T\n\n## UX\n\nNo UI change: it is headless.\n\n### Considered\n\n- /plans/d1: it is \
         about the old flow.\n"
            .to_owned(),
    );
    assert_eq!(first(&found, list).status, Status::Missing);
}

#[test]
fn an_obligation_on_the_plans_paths_is_missing_and_one_in_owed_passes() {
    let list = json!([{"id": "settled", "kind": "obligations"}]);
    let mut found = observed(&json!({}), &[]);
    found.obligations = vec!["/rules/x/docs-owed".to_owned()];
    let check = first(&found, list.clone());
    assert_eq!(check.status, Status::Missing);
    assert_eq!(
        check.detail,
        "these obligations apply to the plan's paths: /rules/x/docs-owed"
    );
    assert_eq!(
        check.remedy.unwrap(),
        "Do the work each obligation names. Once the work is committed, delete each obligation \
         with `board rm <path> -m '<message naming the work>'`; merging the work does not \
         clear it. List an obligation the change leaves owed in the plan's owed field."
    );
    found.owed = vec!["/rules/x/docs-owed".to_owned()];
    assert_eq!(first(&found, list).status, Status::Pass);
}

#[test]
fn an_obligation_listed_only_under_the_old_escape_heading_is_missing_even_with_escape_section() {
    let list = json!([{"id": "settled", "kind": "obligations", "escape_section": "Obligations left owed"}]);
    let mut found = observed(&json!({}), &[]);
    found.obligations = vec!["/rules/x/docs-owed".to_owned()];
    found.plan = Some(
        "# T\n\n## Obligations left owed\n\n- /rules/x/docs-owed, owed to the next step.\n"
            .to_owned(),
    );
    assert_eq!(first(&found, list).status, Status::Missing);
}

#[test]
fn an_obligations_detail_names_ten_obligations_and_counts_the_rest() {
    let list = json!([{"id": "o", "kind": "obligations"}]);
    let mut found = observed(&json!({}), &[]);
    let owed = |count: usize| -> Vec<String> {
        (0..count).map(|at| format!("/rules/x/o{at:02}")).collect()
    };
    found.obligations = owed(10);
    let ten = first(&found, list.clone()).detail;
    assert!(ten.ends_with("/rules/x/o09"), "{ten}");
    found.obligations = owed(12);
    let twelve = first(&found, list).detail;
    assert!(twelve.ends_with("/rules/x/o09, and 2 more"), "{twelve}");
}

#[test]
fn an_obligations_check_on_a_change_with_a_head_fails_when_git_cannot_list_its_diff() {
    let list = json!([{"id": "o", "kind": "obligations"}]);
    let mut headed = observed(&json!({"head": HEAD}), &[]);
    headed.diff_unread = Some("`git diff` failed in /r".to_owned());
    let check = first(&headed, list.clone());
    assert_eq!(
        (check.status, check.detail.as_str(), check.code),
        (
            Status::Missing,
            "`git diff` failed in /r",
            "changes::gate::unmet"
        )
    );
    let mut headless = observed(&json!({}), &[]);
    headless.diff_unread = Some("the change records no head commit".to_owned());
    assert_eq!(first(&headless, list).status, Status::Pass);
}

#[test]
fn a_missing_check_with_blocks_false_never_blocks_and_its_blocking_twin_does() {
    let mut found = observed(&json!({}), &[]);
    found.obligations = vec!["/rules/x/docs-owed".to_owned()];
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
fn controlled_paths_pass_when_no_named_rule_exists_and_say_so_and_miss_an_edit_a_rule_lists() {
    let list =
        json!([{"id": "ui-paths", "kind": "rule-paths", "rules": ["/rules/{repo}/ui-paths"]}]);
    let mut found = observed(&json!({}), &[]);
    found.edited = vec!["/repos/x/ui/a.rs".to_owned()];
    let none = first(&found, list.clone());
    assert_eq!(none.status, Status::Pass);
    assert_eq!(
        none.detail,
        "no rule /rules/x/ui-paths exists, so the board names no such paths and the check \
         passes"
    );
    found.controlled.insert(
        "/rules/x/ui-paths".to_owned(),
        Some(vec!["/repos/x/ui/a.rs".to_owned()]),
    );
    let touched = first(&found, list.clone());
    assert_eq!(touched.status, Status::Missing);
    assert_eq!(
        touched.detail,
        "the change's diff edits files that /rules/x/ui-paths list: /repos/x/ui/a.rs"
    );
    found
        .controlled
        .insert("/rules/x/ui-paths".to_owned(), Some(Vec::new()));
    let clear = first(&found, list);
    assert_eq!((clear.status, clear.detail), (Status::Pass, String::new()));
}

#[test]
fn a_controlled_paths_detail_names_ten_touched_files_and_counts_the_rest() {
    let list = json!([{"id": "ui-paths", "kind": "rule-paths", "rules": ["/rules/x/ui"]}]);
    let mut found = observed(&json!({}), &[]);
    let files = |count: usize| -> Vec<String> {
        (0..count)
            .map(|at| format!("/repos/x/ui/{at:02}.rs"))
            .collect()
    };
    found
        .controlled
        .insert("/rules/x/ui".to_owned(), Some(files(10)));
    let ten = first(&found, list.clone()).detail;
    assert!(ten.ends_with("/repos/x/ui/09.rs"), "{ten}");
    found
        .controlled
        .insert("/rules/x/ui".to_owned(), Some(files(13)));
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
fn a_waivable_check_passes_only_on_a_decision_naming_it_that_names_a_person_and_no_hook_wrote() {
    let list = json!([{"id": "suite-run", "kind": "run", "scope": "full", "waivable": true}]);
    let found = |fields: &Value| {
        let decided = (
            "/d/w",
            "plans/decision/v1",
            by(json!({"refs": ["/changes/t#suite-run"]}), fields),
        );
        first(&observed(&json!({"head": HEAD}), &[decided]), list.clone())
    };
    let [got, wanted] = judged(|fields| found(fields).status == Status::Pass);
    assert_eq!(got, wanted);
    let missing = found(&json!({"by": {"program": "claude-code"}}));
    assert!(
        missing
            .remedy
            .unwrap()
            .contains("he signs with by.person whose refs name /changes/t#suite-run"),
        "the remedy says how to waive the check"
    );
    let waived = found(&json!({"by": {"person": PERSON}}));
    assert_eq!(
        (waived.status, waived.detail),
        (Status::Pass, "the owner waived it in /d/w".to_owned())
    );
}

#[test]
fn a_waivable_check_passes_on_the_owners_answer_whose_chosen_option_names_it() {
    let list = json!([{"id": "suite-run", "kind": "run", "scope": "full", "waivable": true}]);
    let found = |text: &str, fields: &Value| {
        let options = json!([{"id": "w", "text": text}, {"id": "k", "text": "Keep it."}]);
        let asked = ("/q/w", "plans/question/v1", json!({"options": options}));
        let answered = (
            "/a/w",
            "plans/answer/v1",
            by(json!({"question": "/q/w", "choice": "w"}), fields),
        );
        let observed = observed(&json!({"head": HEAD}), &[asked, answered]);
        first(&observed, list.clone())
    };
    let owner = json!({"by": {"person": PERSON}});
    let waived = found("Waive /changes/t#suite-run", &owner);
    assert_eq!(
        (waived.status, waived.detail),
        (Status::Pass, "the owner waived it in /a/w".to_owned())
    );
    let agent = json!({"by": {"program": "claude-code"}});
    assert_eq!(
        found("Waive /changes/t#suite-run", &agent).status,
        Status::Missing
    );
    let other = found("Waive /changes/t#suite-runs", &owner);
    assert_eq!(other.status, Status::Missing);
    assert!(
        other.remedy.unwrap().contains(
            "or with an attached plans/answer/v1 from the owner whose chosen option, in its \
             attached question, names /changes/t#suite-run"
        ),
        "the remedy names the answer route"
    );
}

#[test]
fn a_check_that_is_not_waivable_ignores_a_decision_naming_it() {
    let list = json!([{"id": "suite-run", "kind": "run", "scope": "full"}]);
    let signed = (
        "/d/w",
        "plans/decision/v1",
        json!({"refs": ["/changes/t#suite-run"], "by": {"person": PERSON}}),
    );
    let found = first(&observed(&json!({"head": HEAD}), &[signed]), list);
    assert_eq!(found.status, Status::Missing);
}

/// The kind `planned-rule-paths` over one rule, applied unconditionally.
fn planned_controlled() -> Value {
    json!([{"id": "declared", "kind": "planned-rule-paths", "rules": ["/rules/{repo}/lifecycle-paths"]}])
}

#[test]
fn planned_controlled_misses_a_rule_the_planned_paths_reach_and_passes_one_they_do_not() {
    let mut found = observed(&json!({}), &[]);
    found.held.insert("/rules/x/lifecycle-paths".to_owned());
    found.on_planned = vec!["/rules/x/lifecycle-paths".to_owned()];
    let (_, status, detail) = run(&found, planned_controlled()).remove(0);
    assert_eq!(status, Status::Missing, "{detail}");
    assert_eq!(detail, "the plan's paths reach /rules/x/lifecycle-paths");
    found.on_planned = vec!["/rules/x/other".to_owned()];
    let (_, status, detail) = run(&found, planned_controlled()).remove(0);
    assert_eq!((status, detail.as_str()), (Status::Pass, ""));
}

/// A flow stored before the rename names `controls` under `/controls/` in a
/// check of the former kind `planned-controlled`, and its check reads the
/// rules the board found as the new flow does.
#[test]
fn a_check_naming_controls_under_controls_gates_as_one_naming_rules_under_rules() {
    let before = json!([{"id": "declared", "kind": "planned-controlled", "controls": ["/controls/{repo}/lifecycle-paths"]}]);
    let mut found = observed(&json!({}), &[]);
    found.held.insert("/rules/x/lifecycle-paths".to_owned());
    for on in ["/rules/x/lifecycle-paths", "/rules/x/other"] {
        found.on_planned = vec![on.to_owned()];
        assert_eq!(
            run(&found, before.clone()),
            run(&found, planned_controlled()),
            "{on}"
        );
    }
}

#[test]
fn planned_controlled_says_no_rule_exists_when_the_board_holds_none() {
    let found = observed(&json!({}), &[]);
    let (_, status, detail) = run(&found, planned_controlled()).remove(0);
    assert_eq!(status, Status::Pass);
    assert_eq!(
        detail,
        "no rule /rules/x/lifecycle-paths exists, so the board names no such paths and the \
         check passes"
    );
}

#[test]
fn planned_controlled_is_missing_with_the_refusal_when_for_paths_fails() {
    let mut found = observed(&json!({}), &[]);
    found.on_planned_unread = Some("rules.for-paths refused the call".to_owned());
    let (_, status, detail) = run(&found, planned_controlled()).remove(0);
    assert_eq!(status, Status::Missing);
    assert_eq!(detail, "rules.for-paths refused the call");
}

#[test]
fn planned_controlled_with_no_rules_says_it_cannot_pass() {
    let list = json!([{"id": "declared", "kind": "planned-rule-paths"}]);
    let (_, status, detail) = run(&observed(&json!({}), &[]), list).remove(0);
    assert_eq!(status, Status::Missing);
    assert!(detail.contains("names no rules"), "{detail}");
}

/// The unsure fact the plan marked: an `exists` check takes a `when` and
/// fills `{change}` in its path.
#[test]
fn an_exists_check_with_a_when_condition_fills_change_in_its_path() {
    let list = json!([{
        "id": "x", "kind": "exists", "path": "{change}/lifecycle",
        "when": {"section_not": {"UX": "^no ui change\\b"}}
    }]);
    let found = observed(&json!({}), &[]);
    assert!(run(&found, list.clone()).is_empty(), "the when left it out");
    let mut applied = observed(&json!({}), &[]);
    applied.plan = Some("# T\n\n## UX\n\nA new screen.\n".to_owned());
    let (_, status, detail) = run(&applied, list.clone()).remove(0);
    assert_eq!(status, Status::Missing);
    assert_eq!(detail, "nothing is at /changes/t/lifecycle");
    applied
        .existing
        .insert("/changes/t/lifecycle".to_owned(), true);
    let (_, status, detail) = run(&applied, list).remove(0);
    assert_eq!((status, detail.as_str()), (Status::Pass, ""));
}

/// The twin of the questions pane's `namesWhole` cases in plans.test.tsx.
#[test]
fn a_path_followed_by_a_fragment_a_query_or_an_extension_is_part_of_a_longer_name() {
    use super::names_whole;
    let mock = "https://claude.ai/artifact/m1";
    for (text, path, whole) in [
        ("Approve /changes/t.", PATH, true),
        ("Approve /changes/t. Then build it.", PATH, true),
        ("Approve /changes/t?", PATH, true),
        ("/changes/t-two", PATH, false),
        ("/changes/t#suite-run", PATH, false),
        ("https://claude.ai/artifact/m1?v=2", mock, false),
        ("Agree https://claude.ai/artifact/m1.png", mock, false),
        ("Agree /mockups/x.html", "/mockups/x", false),
        ("Agree /mockups/x.", "/mockups/x", true),
        ("Waive /changes/t#suite-run", "/changes/t#suite-run", true),
        ("Waive /changes/t#suite-run.", "/changes/t#suite-run", true),
        ("Waive /changes/t#suite-runs", "/changes/t#suite-run", false),
    ] {
        assert_eq!(names_whole(text, path), whole, "{text} names {path}");
    }
}

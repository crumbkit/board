//! The shipped development flow's checks that read the plan's typed
//! answers: each applies when the answer equals the one it names, whatever
//! the plan's prose says.

use serde_json::json;

use super::{
    CONTROL, Given, HEAD, MOCKUP, NEIGHBOURS, PLAN, RUN, UNDERSTOOD, both, given, mockups_list,
    planned_ux, rechecked, run, signed, understanding,
};

fn owned(phase: &'static str, answers: serde_json::Value) -> Given {
    (given(phase).answers(answers)).with(json!({"route": "owner", "head": HEAD}))
}

#[test]
fn the_mockups_and_data_checks_apply_by_the_typed_answers_whatever_the_prose_says() {
    let none = owned(
        "plan",
        json!({"ux": "none", "data": "none", "lifecycle": "none"}),
    );
    let checks = none.statuses();
    assert!(!checks.contains_key("mockups") && !checks.contains_key("data-signed"));
    // The same text, which says No UI change and None, with the other answers.
    let both = owned(
        "plan",
        json!({"ux": "mockups", "data": "stored", "lifecycle": "none"}),
    );
    let checks = both.statuses();
    assert_eq!(checks["mockups"], "missing");
    assert_eq!(checks["data-signed"], "missing");
    // Prose that would have asked for both under the old flow asks for neither.
    let prose = super::planned("A new list view.", "A new table.");
    let checks = none.plan(Some(&prose)).statuses();
    assert!(!checks.contains_key("mockups") && !checks.contains_key("data-signed"));
}

#[test]
fn a_plan_that_answers_mockups_lists_them_even_under_no_ui_change_and_passes_once_agreed() {
    let answers = json!({"ux": "mockups", "data": "none", "lifecycle": "none"});
    let unlisted = owned("plan", answers.clone());
    assert_eq!(
        unlisted.check("mockups").1,
        "the plan answers `ux` mockups and its UX section lists none under a ### Mockups heading"
    );
    let listed = planned_ux(&format!("{}\n{NEIGHBOURS}", mockups_list()));
    let agreed = owned("plan", answers).plan(Some(&listed)).records(both());
    assert_eq!(agreed.statuses()["mockups"], "pass");
    assert!(listed.contains(MOCKUP));
}

#[test]
fn stored_data_needs_the_sign_off_at_plan_and_two_rechecks_at_build() {
    let answers = json!({"ux": "none", "data": "stored", "lifecycle": "none"});
    let signed_off = vec![
        (UNDERSTOOD, understanding(&json!({}))),
        ("/d/s", signed(&[UNDERSTOOD])),
    ];
    assert_eq!(
        owned("plan", answers.clone()).statuses()["data-signed"],
        "missing"
    );
    let plan = owned("plan", answers.clone()).records(signed_off);
    assert_eq!(plan.statuses()["data-signed"], "pass");
    let built = |rechecks: &[bool]| {
        owned("build", answers.clone()).records(vec![
            (RUN, run(&json!({}))),
            (
                UNDERSTOOD,
                understanding(&json!({"rechecks": rechecked(rechecks)})),
            ),
            ("/d/s", signed(&[UNDERSTOOD])),
        ])
    };
    assert_eq!(built(&[true]).statuses()["rechecks"], "missing");
    assert_eq!(built(&[true, true]).statuses()["rechecks"], "pass");
    let none = owned(
        "build",
        json!({"ux": "none", "data": "none", "lifecycle": "none"}),
    );
    assert!(!none.statuses().contains_key("rechecks"));
}

fn lifecycle(phase: &'static str, answer: &str) -> Given {
    let mut change = owned(
        phase,
        json!({"ux": "none", "data": "none", "lifecycle": answer}),
    );
    change.on_planned = vec![CONTROL];
    change.held = vec![CONTROL];
    change
}

#[test]
fn a_lifecycle_of_none_is_checked_against_the_planned_paths_and_a_record_must_exist() {
    let none = lifecycle("plan", "none");
    assert_eq!(
        none.check("lifecycle-declared"),
        (
            "missing",
            "the plan's paths reach /controls/x/lifecycle-paths while it answers lifecycle none"
                .to_owned()
        )
    );
    assert!(!none.statuses().contains_key("lifecycle-record"));
    let remedy = none.remedy("lifecycle-declared");
    assert!(remedy.contains("answer `lifecycle` record"), "{remedy}");
    let mut record = lifecycle("plan", "record");
    assert!(!record.statuses().contains_key("lifecycle-declared"));
    assert_eq!(
        record.check("lifecycle-record"),
        ("missing", "nothing is at /changes/t/lifecycle".to_owned())
    );
    let remedy = record.remedy("lifecycle-record");
    assert!(remedy.contains("answer `lifecycle` none"), "{remedy}");
    record.existing = vec![("/changes/t/lifecycle", true)];
    assert_eq!(record.check("lifecycle-record"), ("pass", String::new()));
}

#[test]
fn the_review_and_land_gates_read_the_controlled_paths_of_each_answer_of_none() {
    let all = owned(
        "review",
        json!({"ux": "none", "data": "none", "lifecycle": "none"}),
    );
    let checks = all.statuses();
    for id in ["ui-paths", "data-paths", "lifecycle-none-paths"] {
        assert!(checks.contains_key(id), "{id}: {checks:?}");
    }
    let other = owned(
        "review",
        json!({"ux": "mockups", "data": "stored", "lifecycle": "record"}),
    );
    let checks = other.statuses();
    for id in ["ui-paths", "data-paths", "lifecycle-none-paths"] {
        assert!(!checks.contains_key(id), "{id}: {checks:?}");
    }
    let mut edited = lifecycle("land", "none");
    edited.records_held = vec!["/changes/t/lifecycle"];
    edited.controlled = vec![(CONTROL, vec!["/repos/x/src/sync.rs"])];
    assert_eq!(edited.statuses()["lifecycle-none-paths-at-land"], "missing");
    let mut record = lifecycle("land", "record");
    record.records_held = vec!["/changes/t/lifecycle"];
    record.controlled = vec![(CONTROL, vec!["/repos/x/src/sync.rs"])];
    assert!(!(record.statuses()).contains_key("lifecycle-none-paths-at-land"));
}

/// A plan written before plans gave typed answers, on a change that follows
/// the shipped flow: the check is missing and says how to give the answer.
#[test]
fn a_check_that_reads_an_answer_the_plan_leaves_out_is_missing_and_says_to_write_the_plan_again() {
    let untyped = owned("plan", json!({"ux": "none", "lifecycle": "none"}));
    let (status, detail) = untyped.check("data-signed");
    assert_eq!(status, "missing");
    assert_eq!(
        detail,
        "the plan gives no typed answer for data (none or stored), which this check reads; write \
         the plan again with `board changes plan` and `answers` on stdin"
    );
    // The twin that gives the answer leaves the check out.
    let typed = owned(
        "plan",
        json!({"ux": "none", "data": "none", "lifecycle": "none"}),
    );
    assert!(!typed.statuses().contains_key("data-signed"));
}

/// A change started before plans gave typed answers keeps the flow version
/// it stored, whose checks read the prose, and its plan gives no answers.
#[test]
fn a_change_on_the_stored_prose_flow_passes_and_fails_by_its_prose_with_no_answers() {
    let in_flight = |plan: &str| {
        (given("plan").prose().plan(Some(plan)))
            .with(json!({"route": "owner"}))
            .statuses()
    };
    let checks = in_flight(PLAN);
    assert!(!checks.contains_key("mockups") && !checks.contains_key("data-signed"));
    assert_eq!(checks["plan-sections"], "pass");
    let checks = in_flight(&super::planned("A new list view.", "A new table."));
    assert_eq!(checks["mockups"], "missing");
    assert_eq!(checks["data-signed"], "missing");
    let review = given("review").prose().statuses();
    for id in ["ui-paths", "data-paths", "lifecycle-none-paths"] {
        assert_eq!(review[id], "pass", "{id}");
    }
}

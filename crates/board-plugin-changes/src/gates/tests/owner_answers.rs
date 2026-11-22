//! The owner-approval check counts the owner's answer from any surface of
//! the owner's, and the owner's Touch ID approval of `changes.approve` for
//! the current plan. Each refusal has an admitted twin.

use board_model::{Code, Fields, Refusal};
use serde_json::json;

use super::{PATH, PERSON, UNMET, answer, given};
use crate::gates::{Approval, Read, Status, gate, reads};

/// The status and detail of the fixture change's owner-approval check at
/// plan, with `records` attached and `approval` read.
fn approval_with(records: super::Records, approval: Approval) -> (Status, String) {
    let mut observed = (given("plan").with(json!({"route": "owner"})))
        .records(records)
        .observed();
    observed.approval = approval;
    let found = gate(&observed);
    let check = found
        .into_iter()
        .find(|each| each.check == "owner-approval");
    let check = check.unwrap();
    (check.status, check.detail)
}

#[test]
fn the_panes_answer_passes_owner_approval_and_its_twin_with_neither_writer_nor_person_does_not() {
    let pane = json!({"by": {"session": "local_1"}, "written_by": "plans/pane-answer"});
    let passed = approval_with(answer(PATH, "a", &pane), Approval::None);
    assert_eq!(passed, (Status::Pass, String::new()));
    let unnamed = json!({"by": {"session": "local_1"}});
    let unmet = approval_with(answer(PATH, "a", &unnamed), Approval::None);
    assert_eq!(unmet, (Status::Missing, UNMET.to_owned()));
    let ask = json!({"by": {"person": PERSON}, "written_by": "claude-code/ask"});
    let asked = approval_with(answer(PATH, "a", &ask), Approval::None);
    assert_eq!(asked.0, Status::Pass);
}

#[test]
fn a_decision_written_by_a_hook_still_does_not_pass_owner_approval_though_an_answer_would() {
    let decision = |fields: serde_json::Value| {
        let mut body = json!({"refs": [PATH], "by": {"person": PERSON}});
        for (key, value) in fields.as_object().unwrap() {
            body[key] = value.clone();
        }
        vec![("/d", ("plans/decision/v1", body))]
    };
    let hooked = approval_with(decision(json!({"written_by": "hook"})), Approval::None);
    assert_eq!(hooked.0, Status::Missing);
    let owners = approval_with(decision(json!({})), Approval::None);
    assert_eq!(owners.0, Status::Pass);
}

#[test]
fn the_owners_signed_approval_passes_owner_approval_and_no_approval_does_not() {
    assert_eq!(
        approval_with(Vec::new(), Approval::Signed),
        (Status::Pass, String::new())
    );
    assert_eq!(
        approval_with(Vec::new(), Approval::None),
        (Status::Missing, UNMET.to_owned())
    );
}

#[test]
fn an_approval_board_could_not_read_does_not_pass_and_the_detail_says_why() {
    let refusal = Refusal::new(
        Code::SettingsUnsigned,
        "the signature is bad",
        &Fields::default(),
    );
    assert_eq!(
        approval_with(Vec::new(), Approval::Unread(Box::new(refusal))),
        (
            Status::Missing,
            format!(
                "{UNMET}; Board could not read that approval: board::settings::unsigned the \
                 signature is bad"
            )
        )
    );
}

#[test]
fn only_a_check_that_counts_answers_reads_the_signed_approval() {
    let observed = given("plan").observed();
    assert!(reads(observed.checks()).contains(&Read::Approval));
    let built = given("build").observed();
    assert!(!reads(built.checks()).contains(&Read::Approval));
}

#[test]
fn an_answer_naming_a_check_after_the_change_does_not_approve_its_plan_and_the_bare_path_does() {
    let owner = json!({"by": {"person": PERSON}});
    let anchored = approval_with(
        answer("/changes/t#design-conformance", "a", &owner),
        Approval::None,
    );
    assert_eq!(anchored, (Status::Missing, UNMET.to_owned()));
    let bare = approval_with(answer(PATH, "a", &owner), Approval::None);
    assert_eq!(bare, (Status::Pass, String::new()));
}

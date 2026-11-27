//! Each owner approval binds to one revision of the plan, and the
//! owner-approval check counts it while the current plan differs from that
//! revision only in routine parts. Each refusal has an admitted twin.

use serde_json::{Value, json};

use super::{
    Given, PATH, PERSON, PLAN, Records, STORES, UNDERSTOOD, approving, revision, understanding,
};
use crate::gates::{Approval, Status, gate};

/// The earlier revision the fixture approvals approve.
const EARLIER: &str = "/changes/t/plans/0";
/// The fixture change's current revision.
const CURRENT: &str = "/changes/t/plans/1";

fn stores() -> Value {
    serde_json::from_str(STORES).unwrap()
}

/// The owner's answer, with its question, choosing the option that
/// approves the change as planned in `revision`.
fn owners_answer(revision: &str) -> Records {
    let options = json!([
        {"id": "a", "text": format!("Approve {PATH} as planned in {revision}.")},
        {"id": "b", "text": "Revise the plan, then ask again."},
    ]);
    let question = json!({"question": "Approve?", "options": options});
    let body = json!({"question": "/q/1", "choice": "a", "by": {"person": PERSON}});
    vec![
        ("/q/1", ("plans/question/v1", question)),
        ("/a/1", ("plans/answer/v1", body)),
    ]
}

/// A plan record of the fixture change whose text is at `<path>.md`, with
/// `answers`, `fields` over it and `records` attached.
fn record(path: &str, answers: &Value, fields: &Value, records: &[&str]) -> Value {
    let mut body = json!({
        "text": format!("{path}.md"), "uses": [], "answers": answers, "records": records,
    });
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

/// The fixture change at plan with `records`, whose earlier revision gave
/// `earlier` and attached `under_earlier`, and whose current revision gives
/// `text` with `answers` and `fields` and holds every record.
fn revised(
    records: Records,
    under_earlier: &[&str],
    earlier: &str,
    text: &str,
    answers: &Value,
    fields: &Value,
) -> Given {
    let every: Vec<&str> = records.iter().map(|(path, _)| *path).collect();
    let revisions = vec![
        revision(
            EARLIER,
            &record(EARLIER, &stores(), &json!({}), under_earlier),
            Some(earlier),
        ),
        revision(
            CURRENT,
            &record(CURRENT, answers, fields, &every),
            Some(text),
        ),
    ];
    (approving("plan").with(json!({"route": "owner"})))
        .answers(answers.clone())
        .plan(Some(text))
        .records(records)
        .revisions(Ok(revisions))
}

/// The owner's answer attached under the earlier revision, which gave the
/// fixture plan, and a current revision giving `text`, `answers` and
/// `fields`.
fn answered_earlier(text: &str, answers: &Value, fields: &Value) -> Given {
    revised(
        owners_answer(EARLIER),
        &["/q/1", "/a/1"],
        PLAN,
        text,
        answers,
        fields,
    )
}

/// The status and detail of the owner-approval check, with `approval` read.
fn approval(given: &Given, approval: Approval) -> (Status, String) {
    let mut observed = given.observed();
    observed.approval = approval;
    let check = (gate(&observed).into_iter())
        .find(|each| each.check == "owner-approval")
        .unwrap();
    (check.status, check.detail)
}

fn owner_approval(given: &Given) -> (Status, String) {
    approval(given, Approval::None)
}

#[test]
fn a_revision_that_changes_the_outcome_makes_the_answer_lapse_and_names_the_revision() {
    let text = PLAN.replace("A suite stops.", "A suite stops and retries.");
    let (status, detail) = owner_approval(&answered_earlier(&text, &stores(), &json!({})));
    assert_eq!(status, Status::Missing);
    assert_eq!(
        detail,
        "the owner approved plans/0 in /a/1, and the current plan changes its Outcome section; \
         ask the owner again with the question `board changes ask` files, or with `board \
         changes approve` and Touch ID"
    );
}

#[test]
fn each_substantive_section_and_answer_makes_the_answer_lapse() {
    let cases = [
        (
            "Retries.",
            "Retries and backoff.",
            "its Out of scope section",
        ),
        (
            "No UI change: suites run headless.",
            "No UI change: suites run in the background.",
            "its UX section",
        ),
        (
            "## Data\n\nNone.",
            "## Data\n\nOne row per run.",
            "its Data section",
        ),
    ];
    for (from, to, named) in cases {
        let text = PLAN.replace(from, to);
        let (status, detail) = owner_approval(&answered_earlier(&text, &stores(), &json!({})));
        assert_eq!(status, Status::Missing, "{named}");
        assert!(detail.contains(&format!("changes {named};")), "{detail}");
    }
    let answers = json!({"ux": "none", "data": "stored", "lifecycle": "record"});
    let (status, detail) = owner_approval(&answered_earlier(PLAN, &answers, &json!({})));
    assert_eq!(status, Status::Missing);
    assert!(
        detail.contains("changes its `lifecycle` answer;"),
        "{detail}"
    );
}

#[test]
fn a_section_the_revision_drops_counts_as_empty_text_and_lapses() {
    let text = PLAN.replace("## Out of scope\n\nRetries.\n\n", "");
    let (status, detail) = owner_approval(&answered_earlier(&text, &stores(), &json!({})));
    assert_eq!(status, Status::Missing);
    assert!(
        detail.contains("changes its Out of scope section;"),
        "{detail}"
    );
}

#[test]
fn routine_rewrites_keep_the_answer_and_the_detail_names_the_approved_revision() {
    let decided = format!("{PLAN}\n## Decisions\n\n- Kept the timeout per suite.\n");
    let tested = format!("{PLAN}\n## Tests\n\n- A suite stops.\n");
    let problem = PLAN.replace("Suites hang.", "Suites hang for hours.");
    let cases = [
        (
            PLAN.to_owned(),
            json!({"paths": ["/repos/x/src/**"]}),
            "its `paths`",
        ),
        (
            PLAN.to_owned(),
            json!({"owed": [{"rule": "/rules/a", "why": "Later."}]}),
            "its `owed`",
        ),
        (
            PLAN.to_owned(),
            json!({"considered": [{"records": ["/r"], "why": "Unrelated."}]}),
            "its `considered`",
        ),
        (tested, json!({}), "its Tests section"),
        (problem, json!({}), "its Problem section"),
        (decided, json!({}), "its Decisions section"),
    ];
    for (text, fields, named) in cases {
        let (status, detail) = owner_approval(&answered_earlier(&text, &stores(), &fields));
        assert_eq!(status, Status::Pass, "{named}: {detail}");
        assert_eq!(
            detail,
            format!(
                "the owner approved plans/0 in /a/1, and the current plan changes only {named} \
                 since, which needs no new approval"
            )
        );
    }
}

#[test]
fn a_whitespace_reflow_of_the_outcome_keeps_the_answer_and_a_changed_data_answer_does_not() {
    let text = PLAN.replace("A suite stops.", "A suite\n   stops.");
    let (status, _) = owner_approval(&answered_earlier(&text, &stores(), &json!({})));
    assert_eq!(status, Status::Pass);
    let answers = json!({"ux": "none", "data": "none", "lifecycle": "none"});
    let changed = answered_earlier(PLAN, &answers, &json!({}));
    let mut observed = changed.observed();
    // A plan answering data none needs no approval, so the gate reads the
    // stored answer while the revision records the changed one.
    observed.answers = serde_json::from_value(stores()).unwrap();
    let check = (gate(&observed).into_iter())
        .find(|each| each.check == "owner-approval")
        .unwrap();
    assert_eq!(check.status, Status::Missing);
    assert!(
        check.detail.contains("changes its `data` answer;"),
        "{}",
        check.detail
    );
}

/// The owner's decision whose refs name the change and `refs`, and that
/// decides `decides` when given.
fn decision(refs: &[&str], decides: Option<&str>) -> Records {
    let refs: Vec<&str> = std::iter::once(PATH).chain(refs.iter().copied()).collect();
    let mut body = json!({"refs": refs, "by": {"person": PERSON}});
    if let Some(decides) = decides {
        body["decides"] = json!(decides);
    }
    vec![("/d/1", ("plans/decision/v1", body))]
}

/// The owner's decision whose refs name the change and that decides
/// `decides`.
fn deciding(decides: &str) -> Records {
    decision(&[], Some(decides))
}

/// "Explicit only": a decision whose refs name the change and the current
/// revision but that decides nothing approves nothing, and its twin that
/// decides the current revision approves it.
#[test]
fn a_decision_whose_refs_name_the_current_revision_approves_only_when_it_decides_it() {
    let content = revised(
        decision(&[CURRENT], None),
        &[],
        PLAN,
        PLAN,
        &stores(),
        &json!({}),
    );
    let (status, detail) = owner_approval(&content);
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with(
            "no plans/decision/v1 that the owner signed with by.person whose refs name \
             /changes/t and that decides a revision of the plan, such as /changes/t/plans/1, \
             or the approval question `board changes ask` filed, is attached"
        ),
        "{detail}"
    );
    let approving = revised(deciding(CURRENT), &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&approving), (Status::Pass, String::new()));
}

/// A decision that decides another change's revision, or a section of this
/// change's plan, approves nothing.
#[test]
fn a_decision_deciding_another_change_s_revision_or_a_section_approves_nothing() {
    for decides in [
        "/changes/other/plans/1",
        "/changes/t/plans/1#data",
        "/changes/t",
    ] {
        let given = revised(deciding(decides), &[], PLAN, PLAN, &stores(), &json!({}));
        assert_eq!(owner_approval(&given).0, Status::Missing, "{decides}");
    }
}

/// The approval question `changes ask` filed for the change.
fn asked(revision: &str, blocks: &str) -> (&'static str, (&'static str, Value)) {
    let options = json!([
        {"id": "approve", "text": format!("Approve {PATH} as planned in {revision}.")},
        {"id": "revise", "text": "Revise the plan, then ask again."},
    ]);
    let question = json!({
        "question": format!("Do you approve the plan {revision} of {PATH} (T)?"),
        "options": options, "blocks": blocks,
    });
    ("/q/ask", ("plans/question/v1", question))
}

/// A decision that decides the approval question `changes ask` filed for
/// the change approves the revision its approving option names. One that
/// decides a question about the plan's content, or an approval question
/// filed for another change, approves nothing.
#[test]
fn a_decision_deciding_the_approval_question_approves_the_revision_its_option_names() {
    let mut records = deciding("/q/ask");
    records.push(asked(EARLIER, PATH));
    let text = PLAN.replace("A suite stops.", "A suite stops and retries.");
    let lapsed = revised(records.clone(), &[], PLAN, &text, &stores(), &json!({}));
    let (status, detail) = owner_approval(&lapsed);
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with("the owner approved plans/0 in /d/1,"),
        "{detail}"
    );
    let held = revised(records, &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&held).0, Status::Pass);
    let mut other = deciding("/q/ask");
    other.push(asked(EARLIER, "/changes/other"));
    let given = revised(other, &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&given).0, Status::Missing);
    let options = json!([
        {"id": "a", "text": format!("Store runs as rows under {PATH}, as {CURRENT} says.")},
        {"id": "b", "text": "Store runs as files."},
    ]);
    let content = json!({"question": "Rows or files?", "options": options, "blocks": PATH});
    let mut about = deciding("/q/ask");
    about.push(("/q/ask", ("plans/question/v1", content)));
    let given = revised(about, &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&given).0, Status::Missing);
}

#[test]
fn a_decision_approving_the_earlier_revision_lapses_on_a_data_change() {
    let text = PLAN.replace("## Data\n\nNone.", "## Data\n\nOne row per run.");
    let earlier = revised(
        deciding(EARLIER),
        &["/d/1"],
        PLAN,
        &text,
        &stores(),
        &json!({}),
    );
    let (status, detail) = owner_approval(&earlier);
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with(
            "the owner approved plans/0 in /d/1, and the current plan changes its Data section;"
        ),
        "{detail}"
    );
}

#[test]
fn a_decision_deciding_a_revision_s_text_file_approves_that_revision_wherever_it_is_attached() {
    let text = PLAN.replace("A suite stops.", "A suite stops and retries.");
    let named = deciding("/changes/t/plans/0.md");
    let given = revised(named, &[], PLAN, &text, &stores(), &json!({}));
    let (status, detail) = owner_approval(&given);
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with("the owner approved plans/0 in /d/1,"),
        "{detail}"
    );
}

#[test]
fn an_answer_whose_option_names_a_revision_approves_that_revision() {
    let options = json!([
        {"id": "a", "text": format!("Approve {PATH} as planned in {EARLIER}.")},
        {"id": "b", "text": "Revise the plan, then ask again."},
    ]);
    let question = json!({"question": "Approve?", "options": options});
    let body = json!({"question": "/q/1", "choice": "a", "by": {"person": PERSON}});
    let records = vec![
        ("/q/1", ("plans/question/v1", question)),
        ("/a/1", ("plans/answer/v1", body)),
    ];
    let text = PLAN.replace("A suite stops.", "A suite stops and retries.");
    let given = revised(records.clone(), &[], PLAN, &text, &stores(), &json!({}));
    assert_eq!(owner_approval(&given).0, Status::Missing);
    let same = revised(records, &[], PLAN, PLAN, &stores(), &json!({}));
    let (status, detail) = owner_approval(&same);
    assert_eq!(status, Status::Pass);
    assert!(
        detail.starts_with("the owner approved plans/0 in /a/1,"),
        "{detail}"
    );
}

#[test]
fn an_approval_naming_a_revision_board_cannot_read_lapses_and_its_twin_passes() {
    let gone = revised(
        deciding("/changes/t/plans/gone"),
        &[],
        PLAN,
        PLAN,
        &stores(),
        &json!({}),
    );
    assert_eq!(
        owner_approval(&gone),
        (
            Status::Missing,
            "the owner approved plans/gone in /d/1, which Board can no longer read; ask the \
             owner again with the question `board changes ask` files, or with `board changes \
             approve` and Touch ID"
                .to_owned()
        )
    );
    let held = revised(deciding(EARLIER), &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&held).0, Status::Pass);
}

#[test]
fn a_refused_listing_of_the_revisions_leaves_the_check_missing_with_the_refusal() {
    let given = (approving("plan").with(json!({"route": "owner"})))
        .records(owners_answer(CURRENT))
        .revisions(Err("changes::board::refused: query failed".to_owned()));
    assert_eq!(
        owner_approval(&given),
        (
            Status::Missing,
            "Board could not list the revisions of the plan of /changes/t, so it cannot tell \
             which one the owner approved: changes::board::refused: query failed"
                .to_owned()
        )
    );
}

#[test]
fn a_touch_id_approval_of_the_earlier_revision_survives_a_path_rewrite_and_lapses_on_ux() {
    let none = revised(
        Vec::new(),
        &[],
        PLAN,
        PLAN,
        &stores(),
        &json!({"paths": ["/x/**"]}),
    );
    let signed = || Approval::Signed(EARLIER.to_owned());
    assert_eq!(
        approval(&none, signed()),
        (
            Status::Pass,
            "the owner approved plans/0 with Touch ID, and the current plan changes only its \
             `paths` since, which needs no new approval"
                .to_owned()
        )
    );
    let text = PLAN.replace("suites run headless", "suites show a spinner");
    let ux = revised(Vec::new(), &[], PLAN, &text, &stores(), &json!({}));
    let (status, detail) = approval(&ux, signed());
    assert_eq!(status, Status::Missing);
    assert!(
        detail.starts_with(
            "the owner approved plans/0 with Touch ID, and the current plan changes its UX section;"
        ),
        "{detail}"
    );
    let current = revised(Vec::new(), &[], PLAN, &text, &stores(), &json!({}));
    assert_eq!(
        approval(&current, Approval::Signed(CURRENT.to_owned())),
        (Status::Pass, String::new())
    );
}

#[test]
fn an_approval_of_the_current_revision_counts_though_another_lapsed() {
    let text = PLAN.replace("A suite stops.", "A suite stops and retries.");
    let given = answered_earlier(&text, &stores(), &json!({}));
    assert_eq!(
        approval(&given, Approval::Signed(CURRENT.to_owned())),
        (Status::Pass, String::new())
    );
}

/// "Explicit only" for answers: an answer whose chosen option names the
/// change but no revision of its plan approves nothing, and its twin naming
/// the current revision approves it.
#[test]
fn an_answer_naming_the_change_but_no_revision_does_not_approve_and_its_twin_does() {
    let options = json!([
        {"id": "a", "text": format!("Store runs as rows under {PATH}.")},
        {"id": "b", "text": "Store runs as files."},
    ]);
    let question = json!({"question": "Rows or files?", "options": options});
    let body = json!({"question": "/q/1", "choice": "a", "by": {"person": PERSON}});
    let content = vec![
        ("/q/1", ("plans/question/v1", question)),
        ("/a/1", ("plans/answer/v1", body)),
    ];
    let given = revised(content, &[], PLAN, PLAN, &stores(), &json!({}));
    assert_eq!(owner_approval(&given).0, Status::Missing);
    let approving = revised(
        owners_answer(CURRENT),
        &[],
        PLAN,
        PLAN,
        &stores(),
        &json!({}),
    );
    assert_eq!(owner_approval(&approving), (Status::Pass, String::new()));
}

/// A Data sign-off whose refs name the change, its understanding check and
/// the current revision, as on the live board, and that decides the
/// understanding check, signs off data-signed and approves no plan; the
/// same decision deciding the current revision approves it.
#[test]
fn a_data_sign_off_naming_the_current_revision_does_not_approve_the_plan() {
    let data = PLAN.replace("## Data\n\nNone.", "## Data\n\nOne row per run.");
    let sign_off = vec![
        (UNDERSTOOD, understanding(&json!({}))),
        (
            "/d/data",
            (
                "plans/decision/v1",
                json!({
                    "decision": "The data the comments store is signed off.",
                    "refs": [PATH, UNDERSTOOD, CURRENT], "by": {"person": PERSON},
                    "decides": UNDERSTOOD,
                }),
            ),
        ),
    ];
    let given = revised(sign_off, &[], &data, &data, &stores(), &json!({}));
    let statuses = given.statuses();
    assert_eq!(
        (statuses["owner-approval"], statuses["data-signed"]),
        ("missing", "pass")
    );
    let plain = revised(deciding(CURRENT), &[], &data, &data, &stores(), &json!({}));
    assert_eq!(plain.statuses()["owner-approval"], "pass");
}

/// A mockup agreement whose refs name the current revision agrees the
/// mockup and approves no plan.
#[test]
fn a_mockup_agreement_naming_the_current_revision_does_not_approve_the_plan() {
    let agreed = vec![(
        "/d/ux",
        (
            "plans/decision/v1",
            json!({
                "decision": "Agreed.", "urls": [super::MOCKUP], "refs": [PATH, CURRENT],
                "by": {"person": PERSON},
            }),
        ),
    )];
    let ux = format!("{}\n{}", super::mockups_list(), super::NEIGHBOURS);
    let text = super::planned(&ux, "None.");
    let given = revised(agreed, &[], &text, &text, &stores(), &json!({}));
    assert_eq!(given.statuses()["owner-approval"], "missing");
}

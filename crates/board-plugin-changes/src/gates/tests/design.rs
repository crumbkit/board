//! The design-conformance checks of the shipped development flow: the plan
//! names the version of each agreed mockup, and the review holds a record
//! that the built page matches that version. Each refusal has an admitted
//! twin.

use serde_json::{Value, json};

use super::{Given, HEAD, MOCKUP, NEIGHBOURS, PERSON, agreed, answered, given, planned_ux};

const VERSION: &str = "1793473476-1c42";
const NOTHING_WRONG: &str = "match";

fn answers() -> Value {
    json!({"ux": "mockups", "data": "none", "lifecycle": "none"})
}

/// A plan whose one mockup is listed as `item`.
fn plan_listing(item: &str) -> String {
    planned_ux(&format!("### Mockups\n\n- {item}\n\n{NEIGHBOURS}"))
}

fn named_version() -> String {
    plan_listing(&format!("The list view: {MOCKUP}, version {VERSION}"))
}

fn at_plan(plan: &str) -> Given {
    (given("plan").answers(answers()).plan(Some(plan)))
        .with(json!({"route": "owner", "head": HEAD}))
        .records(agreed(&[MOCKUP]))
}

/// A conformance record of the fixture mockup's version at the head, with
/// `fields` over it.
fn conformance(fields: &Value) -> (&'static str, Value) {
    let mut body = json!({
        "mockup": MOCKUP, "version": VERSION, "head": HEAD,
        "page": "http://localhost:3000/flows", "verdict": NOTHING_WRONG, "differences": [],
    });
    for (key, value) in fields.as_object().unwrap() {
        body[key] = value.clone();
    }
    ("changes/conformance/v1", body)
}

fn at_review(records: Vec<(&'static str, (&'static str, Value))>) -> Given {
    (given("review")
        .answers(answers())
        .plan(Some(&named_version())))
    .with(json!({"head": HEAD}))
    .records(records)
}

#[test]
fn a_mockup_that_names_no_version_is_missing_and_one_that_names_its_version_passes() {
    let bare = at_plan(&plan_listing(&format!("The list view: {MOCKUP}")));
    assert_eq!(
        bare.check("mockup-versions"),
        (
            "missing",
            format!(
                "this item of the Mockups list names no version of its artifact: The list view: \
                 {MOCKUP}; add `version <id>` to it, or name a /artifacts/<id>/versions/<id> path"
            )
        )
    );
    assert_eq!(bare.code("mockup-versions"), "changes::ux::no_version");
    assert_eq!(
        at_plan(&named_version()).statuses()["mockup-versions"],
        "pass"
    );
    let path = at_plan(&plan_listing("`/artifacts/m1/versions/v7`"))
        .records(agreed(&["/artifacts/m1/versions/v7"]));
    assert_eq!(path.statuses()["mockup-versions"], "pass");
}

#[test]
fn words_after_version_that_hold_no_digit_name_no_version() {
    for item in [
        format!("Mobile version: {MOCKUP}"),
        format!("The version picker: {MOCKUP}"),
    ] {
        let found = at_plan(&plan_listing(&item));
        assert_eq!(
            found.code("mockup-versions"),
            "changes::ux::no_version",
            "{item}"
        );
    }
    let named = at_plan(&plan_listing(&format!(
        "The version picker: {MOCKUP}, version v2"
    )));
    assert_eq!(named.statuses()["mockup-versions"], "pass");
}

#[test]
fn a_decision_that_names_only_the_artifact_agrees_its_versioned_path() {
    let item = "`/artifacts/m1/versions/v7`";
    let artifact = at_plan(&plan_listing(item)).records(agreed(&["/artifacts/m1"]));
    assert_eq!(artifact.statuses()["mockups"], "pass");
    let other = at_plan(&plan_listing(item)).records(agreed(&["/artifacts/m2"]));
    assert_eq!(other.statuses()["mockups"], "missing");
}

#[test]
fn a_change_with_no_ui_has_neither_design_check() {
    let none = given("plan").answers(json!({"ux": "none", "data": "none", "lifecycle": "none"}));
    assert!(!none.statuses().contains_key("mockup-versions"));
    let review =
        given("review").answers(json!({"ux": "none", "data": "none", "lifecycle": "none"}));
    assert!(!review.statuses().contains_key("design-conformance"));
}

#[test]
fn the_review_needs_a_record_that_the_build_matches_the_named_version() {
    let none = at_review(Vec::new());
    assert_eq!(
        none.check("design-conformance"),
        (
            "missing",
            format!(
                "no changes/conformance/v1 record says the built page matches {MOCKUP} version \
                 {VERSION} at {HEAD}"
            )
        )
    );
    let matching = at_review(vec![("/c/1", conformance(&json!({})))]);
    assert_eq!(matching.statuses()["design-conformance"], "pass");
}

#[test]
fn a_difference_fails_the_review_and_the_detail_lists_each_element() {
    let differences = json!([
        {"element": "flow graph", "kind": "missing", "expected": "a node per phase"},
        {"element": "status badge", "kind": "different", "expected": "green", "built": "grey"},
    ]);
    let differs = at_review(vec![(
        "/c/1",
        conformance(&json!({"verdict": "differs", "differences": differences})),
    )]);
    assert_eq!(
        differs.check("design-conformance"),
        (
            "missing",
            format!(
                "the built page differs from {MOCKUP} version {VERSION}: flow graph is missing, \
                 expected a node per phase; status badge is different, expected green, built grey"
            )
        )
    );
    assert_eq!(
        differs.code("design-conformance"),
        "changes::ux::not_conformed"
    );
}

#[test]
fn a_match_that_lists_a_difference_does_not_pass() {
    let listed = json!([{"element": "title", "kind": "missing", "expected": "Flows"}]);
    let found = at_review(vec![("/c/1", conformance(&json!({"differences": listed})))]);
    assert_eq!(
        found.code("design-conformance"),
        "changes::ux::not_conformed"
    );
    let clean = at_review(vec![("/c/1", conformance(&json!({"differences": []})))]);
    assert_eq!(clean.statuses()["design-conformance"], "pass");
}

#[test]
fn a_record_of_another_version_or_another_head_does_not_count() {
    for other in [
        json!({"version": "other"}),
        json!({"mockup": "https://claude.ai/artifact/m9"}),
        json!({"head": "b".repeat(40)}),
    ] {
        let found = at_review(vec![("/c/1", conformance(&other))]);
        assert_eq!(
            found.code("design-conformance"),
            "changes::ux::not_conformed",
            "{other}"
        );
    }
    let same = at_review(vec![("/c/1", conformance(&json!({})))]);
    assert_eq!(same.statuses()["design-conformance"], "pass");
}

#[test]
fn timestamps_with_a_fraction_order_by_their_instant() {
    let listed = json!([{"element": "title", "kind": "missing", "expected": "Flows"}]);
    let differs =
        json!({"verdict": "differs", "differences": listed, "at": "2026-11-19T10:00:00Z"});
    let later = json!({"at": "2026-11-19T10:00:00.5Z"});
    let found = at_review(vec![
        ("/c/1", conformance(&differs)),
        ("/c/2", conformance(&later)),
    ]);
    assert_eq!(found.statuses()["design-conformance"], "pass");
    let earlier = json!({"at": "2026-11-19T09:59:59.9Z"});
    let found = at_review(vec![
        ("/c/1", conformance(&differs)),
        ("/c/2", conformance(&earlier)),
    ]);
    assert_eq!(
        found.code("design-conformance"),
        "changes::ux::not_conformed"
    );
}

#[test]
fn a_later_matching_record_passes_where_an_earlier_one_differed() {
    let listed = json!([{"element": "title", "kind": "missing", "expected": "Flows"}]);
    let found = at_review(vec![
        (
            "/c/1",
            conformance(&json!({"verdict": "differs", "differences": listed})),
        ),
        ("/c/2", conformance(&json!({}))),
    ]);
    assert_eq!(found.statuses()["design-conformance"], "pass");
}

const PLAN_UNVERSIONED: &str = "The list view: https://claude.ai/artifact/m1";

#[test]
fn a_plan_edited_to_drop_the_version_fails_the_review_and_a_versioned_one_passes() {
    let edited = (given("review")
        .answers(answers())
        .plan(Some(&plan_listing(PLAN_UNVERSIONED))))
    .with(json!({"head": HEAD}))
    .records(vec![("/c/1", conformance(&json!({})))]);
    assert_eq!(
        edited.code("design-conformance"),
        "changes::ux::not_conformed"
    );
    assert!(
        edited
            .check("design-conformance")
            .1
            .contains("names no version")
    );
    let kept = at_review(vec![("/c/1", conformance(&json!({})))]);
    assert_eq!(kept.statuses()["design-conformance"], "pass");
}

#[test]
fn the_newest_record_at_the_head_decides() {
    let listed = json!([{"element": "title", "kind": "missing", "expected": "Flows"}]);
    let differs =
        json!({"verdict": "differs", "differences": listed, "at": "2026-11-19T10:00:00Z"});
    let earlier = json!({"at": "2026-11-19T09:00:00Z"});
    let later = json!({"at": "2026-11-19T11:00:00Z"});
    let recovered = at_review(vec![
        ("/c/1", conformance(&earlier)),
        ("/c/2", conformance(&differs)),
        ("/c/3", conformance(&later)),
    ]);
    assert_eq!(recovered.statuses()["design-conformance"], "pass");
    let regressed = at_review(vec![
        ("/c/1", conformance(&earlier)),
        ("/c/2", conformance(&differs)),
    ]);
    assert_eq!(
        regressed.code("design-conformance"),
        "changes::ux::not_conformed"
    );
}

fn answer(by: &Value) -> (&'static str, (&'static str, Value)) {
    (
        "/q/1/answer",
        (
            "plans/answer/v1",
            json!({"question": "/q/1", "words": "Skip it.", "by": by}),
        ),
    )
}

fn unbuilt() -> Value {
    let listed = json!([
        {"element": "graph", "kind": "missing", "expected": "a node per phase", "question": "/q/1"},
    ]);
    json!({"verdict": "differs", "differences": listed})
}

#[test]
fn the_owners_answer_to_the_builders_question_clears_that_difference() {
    let open = at_review(vec![("/c/1", conformance(&unbuilt()))]);
    assert_eq!(
        open.code("design-conformance"),
        "changes::ux::not_conformed"
    );
    let answered = at_review(vec![
        ("/c/1", conformance(&unbuilt())),
        answer(&json!({"person": PERSON})),
    ]);
    assert_eq!(answered.statuses()["design-conformance"], "pass");
    let by_agent = at_review(vec![
        ("/c/1", conformance(&unbuilt())),
        answer(&json!({"program": "claude-code", "session": "s"})),
    ]);
    assert_eq!(
        by_agent.code("design-conformance"),
        "changes::ux::not_conformed"
    );
}

/// The table of who wrote an answer, run through the answer that clears
/// a difference: an answer naming a person, or written through a surface
/// of the owner's, clears it.
#[test]
fn an_answer_from_any_surface_of_the_owners_clears_the_difference_it_answers() {
    let [got, wanted] = answered(|fields| {
        let mut body = json!({"question": "/q/1", "words": "Skip it."});
        for (key, value) in fields.as_object().unwrap() {
            body[key] = value.clone();
        }
        let records = vec![
            ("/c/1", conformance(&unbuilt())),
            ("/q/1/answer", ("plans/answer/v1", body)),
        ];
        at_review(records).statuses()["design-conformance"] == "pass"
    });
    assert_eq!(got, wanted);
}

#[test]
fn an_answer_to_another_question_clears_nothing() {
    let other = (
        "/q/2/answer",
        (
            "plans/answer/v1",
            json!({"question": "/q/2", "words": "Fine.", "by": {"person": PERSON}}),
        ),
    );
    let found = at_review(vec![("/c/1", conformance(&unbuilt())), other]);
    assert_eq!(
        found.code("design-conformance"),
        "changes::ux::not_conformed"
    );
    let own = (
        "/q/1/answer",
        (
            "plans/answer/v1",
            json!({"question": "/q/1", "words": "Fine.", "by": {"person": PERSON}}),
        ),
    );
    let cleared = at_review(vec![("/c/1", conformance(&unbuilt())), own]);
    assert_eq!(cleared.statuses()["design-conformance"], "pass");
}

#[test]
fn a_waiver_the_owner_signed_passes_the_conformance_check() {
    let unwaived = at_review(vec![("/c/1", conformance(&unbuilt()))]);
    assert_eq!(unwaived.statuses()["design-conformance"], "missing");
    let decision = (
        "/d/waive",
        (
            "plans/decision/v1",
            json!({"decision": "Waived.", "refs": ["/changes/t#design-conformance"], "by": {"person": PERSON}}),
        ),
    );
    let waived = at_review(vec![("/c/1", conformance(&unbuilt())), decision]);
    assert_eq!(waived.statuses()["design-conformance"], "pass");
}

/// An attached question whose option `agree` says `text` and whose option
/// `change` names nothing, and an attached answer that chose `choice`, by
/// `by`.
fn asked(text: &str, choice: &str, by: &Value) -> super::Records {
    let options = json!([{"id": "agree", "text": text}, {"id": "change", "text": "Change it."}]);
    let answer = json!({"question": "/q/m", "choice": choice, "words": "Yes.", "by": by});
    vec![
        (
            "/q/m",
            (
                "plans/question/v1",
                json!({"question": "Agree?", "options": options}),
            ),
        ),
        ("/a/m", ("plans/answer/v1", answer)),
    ]
}

#[test]
fn the_owners_answer_whose_chosen_option_names_the_mockup_agrees_it() {
    let owner = json!({"person": PERSON});
    let agreeing = |records| at_plan(&named_version()).records(records).check("mockups");
    let text = format!("Agree the screenshots at {MOCKUP} version {VERSION} as the mockup");
    assert_eq!(
        agreeing(asked(&text, "agree", &owner)),
        ("pass", String::new())
    );
    let unagreed = (
        "missing",
        format!(
            "these mockups have neither an attached plans/decision/v1 the owner signed that \
             names them in its urls or refs, nor an attached plans/answer/v1 from the owner whose \
             chosen option, in its attached question, names them: {MOCKUP}"
        ),
    );
    let agent = json!({"program": "claude-code", "session": "s"});
    assert_eq!(
        agreeing(asked(&text, "agree", &agent)),
        unagreed,
        "no person"
    );
    let alone = asked(&text, "agree", &owner).split_off(1);
    assert_eq!(agreeing(alone), unagreed, "the question is not attached");
    assert_eq!(
        agreeing(asked(&text, "change", &owner)),
        unagreed,
        "the other option"
    );
    for choice in ["none", "zzz"] {
        assert_eq!(
            agreeing(asked(&text, choice, &owner)),
            unagreed,
            "the choice {choice} names no option"
        );
    }
    for other in [
        "Agree https://claude.ai/artifact/m9",
        "Agree https://claude.ai/artifact/m10",
        "Agree https://claude.ai/artifact/m1/",
        "Agree https://claude.ai/artifact/m1?v=2",
    ] {
        assert_eq!(agreeing(asked(other, "agree", &owner)), unagreed, "{other}");
    }
}

#[test]
fn an_answer_naming_the_artifact_agrees_its_versioned_path_and_one_naming_another_does_not() {
    let owner = json!({"person": PERSON});
    let item = "`/artifacts/m1/versions/v7`";
    let found = |text: &str| {
        at_plan(&plan_listing(item))
            .records(asked(text, "agree", &owner))
            .statuses()["mockups"]
    };
    assert_eq!(found("Agree /artifacts/m1 as the mockup"), "pass");
    assert_eq!(found("Agree /artifacts/m1/versions/v7"), "pass");
    assert_eq!(found("Agree /artifacts/m2 as the mockup"), "missing");
}

#[test]
fn the_owners_answer_naming_the_check_after_the_change_waives_conformance_and_the_bare_path_does_not()
 {
    let owner = json!({"person": PERSON});
    let waiving = |text: &str| {
        let mut records = vec![("/c/1", conformance(&unbuilt()))];
        records.extend(asked(text, "agree", &owner));
        at_review(records).check("design-conformance")
    };
    assert_eq!(
        waiving("Waive /changes/t#design-conformance"),
        ("pass", "the owner waived it in /a/m".to_owned())
    );
    let bare = waiving("Waive the design-conformance comparison for /changes/t");
    assert_eq!(bare.0, "missing");
    assert!(
        bare.1.ends_with(
            "; the owner's answer /a/m names /changes/t but not /changes/t#design-conformance, \
             so it does not waive this check"
        ),
        "{}",
        bare.1
    );
    let other = waiving("Waive /changes/t#suite-run");
    assert_eq!(other.0, "missing");
    assert!(!other.1.contains("/a/m"), "{}", other.1);
}

/// The gates read only which paths the chosen option names, never its
/// wording, so the skill's rule is the guard: only the agreeing option names
/// the mockup, and it does not name the bare change path. These cases pin
/// what happens when an option breaks that rule.
#[test]
fn a_chosen_option_counts_for_every_path_it_names_whatever_it_says() {
    let owner = json!({"person": PERSON});
    let both = format!("Agree {MOCKUP} as the mockup for /changes/t");
    let given = at_plan(&named_version()).records(asked(&both, "agree", &owner));
    let statuses = given.statuses();
    assert_eq!(
        (statuses["mockups"], statuses["owner-approval"]),
        ("pass", "pass"),
        "an option naming the mockup and the bare change path also approves the plan"
    );
    let mockup_only = format!("Agree {MOCKUP} as the mockup");
    let given = at_plan(&named_version()).records(asked(&mockup_only, "agree", &owner));
    assert_eq!(given.statuses()["owner-approval"], "missing");
    let revise = format!("Revise {MOCKUP}: make the header blue");
    let given = at_plan(&named_version()).records(asked(&revise, "agree", &owner));
    assert_eq!(
        given.statuses()["mockups"],
        "pass",
        "an option that rejects the mockup but names it agrees it"
    );
}

/// The plan names version 2e27 of the fixture mockup.
fn plan_naming(version: &str) -> String {
    plan_listing(&format!("The install sheet: {MOCKUP} version {version}"))
}

#[test]
fn an_answer_agreeing_another_version_of_the_mockup_does_not_count_and_the_same_version_does() {
    let owner = json!({"person": PERSON});
    let agreeing = |agreed: &str| {
        let text = format!("Agree {MOCKUP} version {agreed} as the mockup");
        at_plan(&plan_naming("1795531196-2e27"))
            .records(asked(&text, "agree", &owner))
            .check("mockups")
    };
    assert_eq!(
        agreeing("1795519815-da06"),
        (
            "missing",
            format!(
                "the owner agreed {MOCKUP} version 1795519815-da06 in /a/m, and the plan names \
                 version 1795531196-2e27; ask the owner to agree version 1795531196-2e27"
            )
        )
    );
    assert_eq!(agreeing("1795531196-2e27"), ("pass", String::new()));
}

#[test]
fn a_decision_agreeing_another_version_does_not_count_and_one_naming_no_version_counts_with_a_note()
{
    let decided = |words: &str| {
        let body = json!({
            "decision": words, "urls": [MOCKUP], "refs": [], "by": {"person": PERSON},
        });
        at_plan(&plan_naming("1795531196-2e27"))
            .records(vec![("/d/ux", ("plans/decision/v1", body))])
            .check("mockups")
    };
    let other = decided(&format!(
        "Sign off {MOCKUP} version 1795519815-da06 as shown."
    ));
    assert_eq!(other.0, "missing");
    assert!(
        other.1.contains("version 1795519815-da06 in /d/ux"),
        "{}",
        other.1
    );
    let same = decided(&format!(
        "Sign off {MOCKUP} version 1795531196-2e27 as shown."
    ));
    assert_eq!(same, ("pass", String::new()));
    assert_eq!(
        decided("Sign off the install sheet as shown."),
        (
            "pass",
            format!(
                "the owner's agreement /d/ux names no version of {MOCKUP}, so it counts for \
                 version 1795531196-2e27, the one the plan names"
            )
        )
    );
}

//! The design-conformance checks of the shipped development flow: the plan
//! names the version of each agreed mockup, and the review holds a record
//! that the built page matches that version. Each refusal has an admitted
//! twin.

use serde_json::{Value, json};

use super::{Given, HEAD, MOCKUP, NEIGHBOURS, agreed, given, planned_ux};

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
    assert_eq!(found.statuses()["design-conformance"], "missing");
}

#[test]
fn a_record_of_another_version_or_another_head_does_not_count() {
    for other in [
        json!({"version": "other"}),
        json!({"mockup": "https://claude.ai/artifact/m9"}),
        json!({"head": "b".repeat(40)}),
    ] {
        let found = at_review(vec![("/c/1", conformance(&other))]);
        assert_eq!(found.statuses()["design-conformance"], "missing", "{other}");
    }
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

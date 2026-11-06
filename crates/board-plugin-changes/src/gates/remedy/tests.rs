//! Filling a flow's placeholders, and the remedy a missing check gives.

use super::{Values, fill, of};
use crate::flow;

fn values(head: Option<&str>) -> Values {
    [
        ("change", Some("/changes/t".to_owned())),
        ("name", Some("t".to_owned())),
        ("mount", Some("/repos/x".to_owned())),
        ("repo", Some("x".to_owned())),
        ("plan", Some("/changes/t/plans/1".to_owned())),
        ("head", head.map(str::to_owned)),
        ("suite", Some("/suites/checks".to_owned())),
        ("draft", None),
        ("board", Some("/b".to_owned())),
    ]
}

#[test]
fn a_placeholder_with_a_value_is_filled_and_other_braces_stay() {
    assert_eq!(
        fill(
            "Check `{head}` on {change}, then {unknown}.",
            &values(Some("abc"))
        ),
        "Check `abc` on /changes/t, then {unknown}."
    );
}

#[test]
fn a_placeholder_with_no_value_is_left_out_with_its_empty_code_span_and_spare_spaces() {
    assert_eq!(
        fill("Check `{head}` on {change}, then {unknown}.", &values(None)),
        "Check on /changes/t, then {unknown}."
    );
    assert_eq!(fill("Run it on {head}.", &values(None)), "Run it on.");
}

fn check(list: serde_json::Value) -> flow::Check {
    serde_json::from_value(list).unwrap()
}

#[test]
fn a_flows_remedy_wins_over_its_kinds_and_a_waivable_check_says_how_to_waive_it() {
    let own = check(serde_json::json!({"id": "t", "kind": "tasks", "remedy": "Close {name}."}));
    assert_eq!(of(&own, &values(None)), "Close t.");
    let kinds = check(serde_json::json!({"id": "t", "kind": "tasks"}));
    assert!(of(&kinds, &values(None)).starts_with("Finish each open task"));
    let waivable = check(serde_json::json!({"id": "t", "kind": "tasks", "waivable": true}));
    assert!(of(&waivable, &values(None)).ends_with(
        "The owner may waive this check with a plans/decision/v1 he signs whose refs name \
             /changes/t#t, attached with `board call changes.attach --path /changes/t --ref \
             <decision>`."
    ));
}

#[test]
fn an_owner_check_says_only_the_owner_can_provide_it_and_an_agent_check_does_not() {
    let owner = check(
        serde_json::json!({"id": "a", "kind": "records", "type": "plans/decision/v1", "actor": "owner"}),
    );
    assert!(of(&owner, &values(None)).starts_with("Only the owner can provide this"));
    let agent =
        check(serde_json::json!({"id": "a", "kind": "records", "type": "plans/decision/v1"}));
    assert!(of(&agent, &values(None)).starts_with("Write the record"));
}

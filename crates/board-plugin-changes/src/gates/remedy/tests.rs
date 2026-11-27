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
        "The owner may waive this check with a plans/decision/v1 he signs with by.person whose \
             refs name /changes/t#t, or with an attached plans/answer/v1 from the owner whose \
             chosen option, in its attached question, names /changes/t#t; attach the decision, or \
             the answer and its question, with `board changes attach --path /changes/t --ref \
             <path>`."
    ));
}

#[test]
fn the_mockups_remedy_asks_for_the_owners_decision_with_by_person_or_answer() {
    let mockups = check(serde_json::json!({"id": "m", "kind": "mockups"}));
    let said = of(&mockups, &values(None));
    assert!(
        said.contains("attach the owner's plans/decision/v1 with by.person that names each"),
        "{said}"
    );
    assert!(
        said.contains(
            "or the owner's plans/answer/v1 and its plans/question/v1, whose chosen option names \
             each"
        ),
        "{said}"
    );
}

#[test]
fn the_suite_run_remedy_names_board_github_ci_and_a_local_run() {
    let run = check(serde_json::json!({"id": "suite-run", "kind": "run"}));
    assert_eq!(
        of(&run, &values(Some("abc"))),
        "Once CI has passed on abc, record its check runs as a run of /suites/checks with \
         `board github ci --change /changes/t`, which also attaches the run. Or run \
         /suites/checks on abc yourself, write the run with `board put`, and attach it with \
         `board changes attach --path /changes/t --ref <run>`. When the change names no suite, \
         name one with `board changes attach --path /changes/t --suite /suites/<name>`."
    );
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

#[test]
fn an_owner_approval_check_says_to_attach_the_owners_answer_and_one_with_type_refs_does_not() {
    let approval = |refs: serde_json::Value| {
        check(serde_json::json!({
            "id": "a", "kind": "records", "type": "plans/decision/v1", "actor": "owner",
            "where": {"owner_signed": true, "refs": refs}
        }))
    };
    let said = of(&approval(serde_json::json!(["plan"])), &values(None));
    assert_eq!(
        said,
        "Ask the owner with /ask, with /changes/t in the text of the option that approves it \
         and in no other option, because any chosen option that names it counts. Then attach the question and the owner's answer, each with `board changes attach \
         --path /changes/t --ref <path>`. A plans/decision/v1 the owner signs with by.person \
         whose refs name the plan revision it approves, such as /changes/t/plans/1, also \
         counts; a decision whose refs name no revision is about the plan's content and \
         approves nothing. The owner's Touch ID approval of \
         changes.approve for the current plan also counts: run `board call changes.approve \
         --path /changes/t` and approve the effect it names."
    );
    let typed = of(
        &approval(serde_json::json!(["evals/eval/v1"])),
        &values(None),
    );
    assert_eq!(
        typed,
        "Only the owner can provide this: ask the owner, then write the record the detail \
         describes and attach it with `board changes attach --path /changes/t --ref <path>`."
    );
}

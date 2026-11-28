use super::*;

fn check(text: Value) -> flow::Check {
    serde_json::from_value(text).unwrap()
}

#[test]
fn only_kinds_that_read_the_plan_or_the_change_record_are_judged_ahead() {
    let ahead: Vec<&str> = [
        "sections",
        "considered",
        "field",
        "exists",
        "rule-paths",
        "planned-rule-paths",
    ]
    .into_iter()
    .filter(|kind| judged_ahead(&check(json!({"id": "c", "kind": kind})).kind))
    .collect();
    assert_eq!(ahead.len(), 6, "{ahead:?}");
    for later in [
        "run",
        "records",
        "obligations",
        "paths",
        "tasks",
        "understood",
        "mockups",
        "flow-valid",
        "unknown-kind",
    ] {
        let each = check(json!({"id": "c", "kind": later}));
        assert!(!judged_ahead(&each.kind), "{later}");
    }
}

#[test]
fn a_later_check_is_read_for_by_its_when_alone() {
    let each = check(json!({
        "id": "suite-run", "kind": "run", "scope": "full", "waivable": true,
        "when": {"answers": {"ux": "none"}},
    }));
    let probe = when_only(&each);
    assert_eq!(probe.kind.known(), Some(&Kind::Field));
    assert_eq!((probe.field, probe.waivable), (None, None));
    assert!(probe.when.is_some());
    assert_eq!(probe.id, "suite-run");
    // The probe reads what its `when` needs, the plan, and nothing a run
    // would read.
    let reads = gates::reads(std::slice::from_ref(&probe));
    assert_eq!(reads, [gates::Read::Plan].into());
}

#[test]
fn the_refusal_names_the_flows_phases_as_prose() {
    let phases = ["plan", "build", "review", "land"].map(str::to_owned);
    let refused = bad_through("/changes/t", "nowhere", &phases);
    assert_eq!(refused.code, "changes::next::bad_through");
    assert!(
        refused
            .message
            .ends_with("name one of plan, build, review and land"),
        "{}",
        refused.message
    );
    let none = bad_through("/changes/t", "land", &[]);
    assert!(none.message.contains("follows no flow"), "{}", none.message);
    assert_eq!(listed(&["a".to_owned()]), "a");
    assert_eq!(listed(&["a".to_owned(), "b".to_owned()]), "a and b");
}

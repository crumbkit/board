use super::*;

fn renamed_text(given: &str) -> String {
    text(given, &BTreeMap::new())
}

#[test]
fn text_renames_old_paths_capabilities_commands_and_types_and_nothing_else() {
    for (given, want) in [
        ("/controls/x/a", "/rules/x/a"),
        (
            "see /controls/x/a, /controls/y.",
            "see /rules/x/a, /rules/y.",
        ),
        ("`board ls /controls`", "`board ls /rules`"),
        ("/repos/team/controls/auth/a", "/repos/team/rules/auth/a"),
        (
            "/repos/board/controls/rules/agents",
            "/repos/board/rules/agents",
        ),
        (
            "/repos/board/controls/budgets/docs",
            "/repos/board/rules/budgets/docs",
        ),
        ("`controls/rules/agents.json`", "`rules/agents.json`"),
        (
            "board call controls.for-paths and controls.check",
            "board call rules.for-paths and rules.check",
        ),
        (
            "board controls check --path .",
            "board rules check --path .",
        ),
        (
            "a controls/control/v1 and a controls/source/v1",
            "a rules/rule/v1 and a rules/source/v1",
        ),
        ("the controls plugin", "the controls plugin"),
        ("/changes/x/controls/a", "/changes/x/controls/a"),
        (
            "https://github.com/o/controls/pull/1",
            "https://github.com/o/controls/pull/1",
        ),
        ("/controls.json", "/controls.json"),
        ("controls::rule::x", "controls::rule::x"),
    ] {
        assert_eq!(renamed_text(given), want, "{given}");
    }
}

#[test]
fn a_record_takes_its_new_type_and_field_names() {
    let none = BTreeMap::new();
    let rule = json!({"$type": CONTROL_TYPE, "requirement": "See /controls/x/b.", "rule": {"check": "version"}});
    assert_eq!(
        rewritten(Some(CONTROL_TYPE), rule, &none),
        json!({"$type": "rules/rule/v1", "requirement": "See /rules/x/b.", "checker": {"check": "version"}})
    );
    let verdict = json!({"verdicts": [{"control": "/controls/a", "rule": "pattern"}, {"rule": "/rules/b", "checker": "judged"}]});
    assert_eq!(
        rewritten(Some("boundary/crossing/v1"), verdict, &none),
        json!({"verdicts": [{"rule": "/rules/a", "checker": "pattern"}, {"rule": "/rules/b", "checker": "judged"}]})
    );
    let boundary = json!({"checks": [{"control": "/controls/a"}]});
    assert_eq!(
        rewritten(Some("boundary/boundary/v1"), boundary, &none),
        json!({"checks": [{"rule": "/rules/a"}]})
    );
    let plan = json!({"owed": [{"control": "/controls/a", "why": "w"}], "paths": ["/repos/board/controls/**"]});
    assert_eq!(
        rewritten(Some("changes/plan/v1"), plan, &none),
        json!({"owed": [{"rule": "/rules/a", "why": "w"}], "paths": ["/repos/board/rules/**"]})
    );
    let flow = json!({"phases": [{"checks": [{"kind": "planned-controlled", "controls": ["/controls/{repo}/x"]}, {"kind": "paths"}]}]});
    assert_eq!(
        rewritten(Some(FLOW_TYPE), flow, &none),
        json!({"phases": [{"checks": [{"kind": "planned-rule-paths", "rules": ["/rules/{repo}/x"]}, {"kind": "paths"}]}]})
    );
}

#[test]
fn a_flow_version_named_anywhere_becomes_its_new_version() {
    let flows = BTreeMap::from([(
        "/flows/development/a9df0910bd037be9".to_owned(),
        "/flows/development/0123456789abcdef".to_owned(),
    )]);
    let change = json!({"flow": "/flows/development/a9df0910bd037be9"});
    assert_eq!(
        rewritten(Some("changes/change/v2"), change, &flows),
        json!({"flow": "/flows/development/0123456789abcdef"})
    );
}

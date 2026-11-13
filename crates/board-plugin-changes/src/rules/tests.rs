use serde_json::json;

use super::*;

#[test]
fn the_rules_plugin_output_reads_in_the_new_names() {
    let output = json!({"rules": [
        {"rule": "/rules/docs", "summary": "Say what exists.", "matched": ["/a"], "close": "Delete it."},
        {"rule": "/rules/rust", "summary": "Errors are typed."},
    ]});
    let found = found_in(&output).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|each| (each.rule.as_str(), each.close.as_deref()))
            .collect::<Vec<_>>(),
        [("/rules/docs", Some("Delete it.")), ("/rules/rust", None)]
    );
    assert_eq!(listed(&output).unwrap().len(), 2);
}

#[test]
fn output_in_the_former_names_is_refused_and_never_read_as_no_rules() {
    let former =
        json!({"controls": [{"control": "/controls/docs", "summary": "Say what exists."}]});
    assert_eq!(
        listed(&former).unwrap_err().code,
        "changes::rules::unreadable"
    );
    assert_eq!(
        found_in(&former).unwrap_err().code,
        "changes::rules::unreadable"
    );
    let half = json!({"rules": [{"control": "/controls/docs", "summary": "Say what exists."}]});
    assert_eq!(
        found_in(&half).unwrap_err().code,
        "changes::rules::unreadable"
    );
}

#[test]
fn the_brief_refuses_a_half_renamed_rule_and_lists_a_whole_one() {
    let whole = json!({"rules": [{"rule": "/rules/docs", "summary": "Say what exists."}]});
    assert_eq!(listed(&whole).unwrap().len(), 1);
    for half in [
        json!({"rules": [{"control": "/controls/docs", "summary": "Say what exists."}]}),
        json!({"rules": [{"rule": "/rules/docs"}]}),
    ] {
        assert_eq!(
            listed(&half).unwrap_err().code,
            "changes::rules::unreadable"
        );
    }
}

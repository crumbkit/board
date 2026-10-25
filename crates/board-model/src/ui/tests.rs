use serde_json::json;

use super::*;

fn view(kind: &str, second: &str) -> Value {
    json!({"$type": VIEW_TYPE, "slots": [
        {"name": "summary", "kind": kind},
        {"name": second, "kind": "record"},
    ]})
}

#[test]
fn a_view_naming_a_kind_outside_the_catalogue_is_refused_and_a_known_kind_is_admitted() {
    let refused = View::from_value(&view("chart", "task")).unwrap_err();
    assert_eq!(refused.code(), Code::UiUnknownKind);
    assert_eq!(refused.pointer().as_deref(), Some("/slots/0/kind"));

    let admitted = View::from_value(&view("markdown", "task")).unwrap();
    assert_eq!(
        admitted.slots,
        [
            Slot {
                name: "summary".to_owned(),
                kind: ComponentKind::Markdown
            },
            Slot {
                name: "task".to_owned(),
                kind: ComponentKind::Record
            },
        ]
    );
}

#[test]
fn a_view_naming_a_slot_twice_is_refused_and_distinct_names_are_admitted() {
    let refused = View::from_value(&view("markdown", "summary")).unwrap_err();
    assert_eq!(refused.code(), Code::RecordInvalid);
    assert_eq!(refused.pointer().as_deref(), Some("/slots/1/name"));
    assert!(View::from_value(&view("markdown", "notes")).is_ok());
}

#[test]
fn a_view_with_a_field_it_does_not_know_is_refused_and_its_twin_is_admitted() {
    let mut value = view("markdown", "task");
    assert!(View::from_value(&value).is_ok());
    value["title"] = json!("Review");
    assert_eq!(
        View::from_value(&value).unwrap_err().code(),
        Code::RecordInvalid
    );
}

fn binding(fill: &Value) -> Value {
    json!({"$type": BINDING_TYPE, "view": "/views/review", "slots": {"summary": fill}})
}

#[test]
fn a_fill_holding_both_a_ref_and_data_is_refused_and_one_holding_either_is_admitted() {
    let both = binding(&json!({"ref": "/tasks/a", "data": {}}));
    assert_eq!(
        Binding::from_value(&both).unwrap_err().code(),
        Code::RecordInvalid
    );

    let by_ref = Binding::from_value(&binding(&json!({"ref": "/tasks/a"}))).unwrap();
    assert_eq!(
        by_ref.slots["summary"],
        Fill::Ref("/tasks/a".parse().unwrap())
    );
    let by_data = Binding::from_value(&binding(&json!({"data": {"markdown": "x"}}))).unwrap();
    assert_eq!(
        by_data.slots["summary"],
        Fill::Data(json!({"markdown": "x"}))
    );
}

#[test]
fn a_ref_that_is_not_a_logical_path_is_refused_and_one_that_is_is_admitted() {
    let malformed = binding(&json!({"ref": "tasks/a"}));
    assert_eq!(
        Binding::from_value(&malformed).unwrap_err().code(),
        Code::RecordInvalid
    );
    assert!(Binding::from_value(&binding(&json!({"ref": "/tasks/a"}))).is_ok());
}

fn derived(from: &Value) -> Value {
    json!({
        "$type": DERIVED_TYPE,
        "from": from,
        "agent": "claude-code",
        "model": "claude-opus-5-5",
        "markdown": "The change is small.",
    })
}

#[test]
fn derived_content_citing_no_record_is_refused_and_content_citing_one_is_admitted() {
    let refused = check_derived(&derived(&json!([]))).unwrap_err();
    assert_eq!(refused.code(), Code::RecordInvalid);
    assert_eq!(refused.pointer().as_deref(), Some("/from"));
    assert!(check_derived(&derived(&json!(["/tasks/a"]))).is_ok());
}

#[test]
fn derived_content_without_its_model_is_refused_and_its_twin_is_admitted() {
    let mut value = derived(&json!(["/tasks/a"]));
    assert!(check_derived(&value).is_ok());
    value.as_object_mut().unwrap().remove("model");
    assert_eq!(
        check_derived(&value).unwrap_err().code(),
        Code::RecordInvalid
    );
}

#[test]
fn every_kind_reads_back_by_its_name_and_its_template_is_a_fill() {
    for kind in ComponentKind::ALL {
        assert_eq!(ComponentKind::parse(kind.name()), Some(kind));
        let fill: Fill = serde_json::from_str(kind.template()).unwrap();
        assert!(kind.schema().is_object(), "{}", kind.name());
        assert!(matches!(fill, Fill::Ref(_) | Fill::Data(_)));
    }
    assert_eq!(ComponentKind::parse("chart"), None);
}

#[test]
fn only_the_ui_types_board_knows_are_checked_here() {
    assert!(check(VIEW_TYPE, &view("markdown", "task")).unwrap().is_ok());
    assert!(check("ui/chart/v1", &json!({})).is_none());
}

use serde_json::json;

use crate::{Schema, Unexpected};

/// A decision: closed at its root and in each option, with a `$ref` to the
/// option's definition.
fn decision() -> Schema {
    Schema::compile(json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["decision"],
        "properties": {
            "$type": {"type": "string"},
            "decision": {"type": "string"},
            "alternatives": {"type": "array", "items": {"type": "string"}},
            "options": {"type": "array", "items": {"$ref": "#/$defs/Option"}},
            "open": {"type": "object"}
        },
        "$defs": {
            "Option": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"id": {"type": "string"}, "label": {"type": "string"}}
            }
        }
    }))
    .unwrap()
}

fn unexpected(at: &str, name: &str, near: Option<&str>) -> Unexpected {
    Unexpected {
        at: at.to_owned(),
        name: name.to_owned(),
        near: near.map(str::to_owned),
    }
}

#[test]
fn a_field_a_closed_schema_does_not_name_is_unexpected_at_any_depth_in_pointer_order() {
    let record = json!({
        "decision": "d",
        "summary": "s",
        "options": [{"id": "a", "note": "n"}]
    });
    assert_eq!(
        decision().unexpected(&record),
        vec![
            unexpected("/options/0", "note", None),
            unexpected("", "summary", None),
        ]
    );
}

#[test]
fn a_record_of_named_fields_and_an_open_object_has_nothing_unexpected() {
    let record = json!({"decision": "d", "options": [{"id": "a"}], "open": {"any": 1}});
    assert_eq!(decision().unexpected(&record), Vec::new());
}

#[test]
fn a_field_one_edit_from_a_named_one_is_near_it() {
    let record = json!({"decision": "d", "alternative": ["x"], "options": [{"labl": "x"}]});
    assert_eq!(
        decision().unexpected(&record),
        vec![
            unexpected("", "alternative", Some("alternatives")),
            unexpected("/options/0", "labl", Some("label")),
        ]
    );
}

#[test]
fn an_unexpected_field_names_its_pointer_with_its_name_escaped() {
    let field = unexpected("/options/0", "a/b~c", None);
    assert_eq!(field.pointer(), "/options/0/a~1b~0c");
    assert_eq!(unexpected("", "summary", None).pointer(), "/summary");
}

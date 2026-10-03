use serde_json::{Value, json};

use super::*;

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "tasks",
        "version": "0.1.0",
        "description": "Open tasks and their due dates.",
        "run": ["python3", "-m", "board_tasks"],
        "metadata": {"area": "planning"},
        "extract": ["lr/task/v1"],
        "capabilities": [{
            "name": "due",
            "description": "Open tasks due before a date.",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "metadata": {}
        }]
    })
}

fn parse(value: &Value) -> Result<Manifest, ManifestError> {
    Manifest::parse("tasks", value.to_string().as_bytes())
}

/// The manifest with `pointer` set to `value`.
fn with(pointer: &str, value: Value) -> Value {
    let mut manifest = manifest();
    *manifest.pointer_mut(pointer).unwrap() = value;
    manifest
}

fn refused(value: &Value) -> ManifestError {
    let error = parse(value).unwrap_err();
    assert_eq!(error.code(), Code::PluginBadManifest);
    error
}

#[test]
fn a_full_manifest_is_admitted_with_its_schemas_compiled() {
    let manifest = parse(&manifest()).unwrap();
    assert_eq!(manifest.name.as_str(), "tasks");
    assert_eq!(manifest.version.as_str(), "0.1.0");
    assert_eq!(manifest.run, ["python3", "-m", "board_tasks"]);
    assert_eq!(manifest.extract[0].as_str(), "lr/task/v1");
    assert_eq!(manifest.capabilities[0].name.as_str(), "due");
    assert_eq!(
        manifest.capabilities[0].input.document(),
        &json!({"type": "object"})
    );
}

#[test]
fn metadata_extract_and_capabilities_may_be_left_out() {
    let mut value = manifest();
    for key in ["metadata", "extract", "capabilities"] {
        value.as_object_mut().unwrap().remove(key);
    }
    let manifest = parse(&value).unwrap();
    assert!(manifest.metadata.is_empty() && manifest.extract.is_empty());
    assert!(manifest.capabilities.is_empty());
}

#[test]
fn a_manifest_that_is_not_strict_json_is_refused() {
    let text = manifest().to_string();
    let duplicated = text.replacen('{', r#"{"name": "tasks", "#, 1);
    assert!(matches!(
        Manifest::parse("tasks", duplicated.as_bytes()).unwrap_err(),
        ManifestError::Json {
            source: JsonError::DuplicateKey { .. }
        }
    ));
    assert!(Manifest::parse("tasks", text.as_bytes()).is_ok());
}

#[test]
fn an_unknown_field_is_refused() {
    let mut value = manifest();
    value["colour"] = json!("blue");
    assert!(matches!(refused(&value), ManifestError::Shape { .. }));
    let mut value = manifest();
    value["capabilities"][0]["colour"] = json!("blue");
    assert!(matches!(refused(&value), ManifestError::Shape { .. }));
    value["capabilities"][0]
        .as_object_mut()
        .unwrap()
        .remove("colour");
    assert!(parse(&value).is_ok());
}

#[test]
fn a_manifest_of_another_type_is_refused() {
    let value = with("/$type", json!("board/plugin-manifest/v2"));
    assert!(matches!(refused(&value), ManifestError::Type { .. }));
}

#[test]
fn a_name_other_than_its_directory_is_refused() {
    assert!(matches!(
        Manifest::parse("todo", manifest().to_string().as_bytes()).unwrap_err(),
        ManifestError::Directory { .. }
    ));
    assert!(Manifest::parse("tasks", manifest().to_string().as_bytes()).is_ok());
}

#[test]
fn names_are_lowercase_ascii_letters_digits_and_hyphens() {
    for good in ["tasks", "reading-list", "v2", "-"] {
        assert!(good.parse::<PluginName>().is_ok(), "{good}");
        assert!(good.parse::<CapabilityName>().is_ok(), "{good}");
    }
    for bad in ["", "Tasks", "reading_list", "tasks.due", "ä", "a b"] {
        assert!(bad.parse::<PluginName>().is_err(), "{bad}");
        assert!(bad.parse::<CapabilityName>().is_err(), "{bad}");
    }
    assert!(matches!(
        refused(&with("/capabilities/0/name", json!("Due"))),
        ManifestError::Shape { .. }
    ));
    assert!(parse(&with("/capabilities/0/name", json!("due-2"))).is_ok());
}

#[test]
fn a_version_is_three_parts_of_digits() {
    for good in ["0.1.0", "10.20.30", "01.0.0"] {
        assert!(parse(&with("/version", json!(good))).is_ok(), "{good}");
    }
    for bad in ["0.1", "0.1.0.0", "0.1.x", "0..1", "v0.1.0", "0.1.0-rc1", ""] {
        assert!(
            matches!(
                refused(&with("/version", json!(bad))),
                ManifestError::Shape { .. }
            ),
            "{bad}"
        );
    }
}

#[test]
fn an_empty_run_is_refused() {
    assert!(matches!(
        refused(&with("/run", json!([]))),
        ManifestError::EmptyRun
    ));
    assert!(parse(&with("/run", json!(["./run"]))).is_ok());
}

#[test]
fn a_capability_offered_twice_is_refused() {
    let capability = manifest()["capabilities"][0].clone();
    let mut other = capability.clone();
    other["name"] = json!("overdue");
    assert!(parse(&with("/capabilities", json!([capability, other]))).is_ok());
    assert!(matches!(
        refused(&with("/capabilities", json!([capability, capability]))),
        ManifestError::DuplicateCapability { .. }
    ));
}

#[test]
fn a_schema_is_compiled_under_a_mounted_boards_rules() {
    let draft7 = json!({"$schema": "http://json-schema.org/draft-07/schema#"});
    assert!(matches!(
        refused(&with("/capabilities/0/output_schema", draft7)),
        ManifestError::Schema {
            side: Side::Output,
            ..
        }
    ));
    let remote = json!({"$ref": "https://example.com/schema.json"});
    assert!(matches!(
        refused(&with("/capabilities/0/input_schema", remote)),
        ManifestError::Schema {
            side: Side::Input,
            ..
        }
    ));
    let local = json!({"$defs": {"a": {"type": "string"}}, "$ref": "#/$defs/a"});
    assert!(parse(&with("/capabilities/0/input_schema", local)).is_ok());
}

fn offered(value: &Value, capability: Option<&str>, filters: &[&str]) -> Vec<String> {
    let mut manifest = parse(value).unwrap();
    let capability = capability.map(|name| name.parse().unwrap());
    let filters: Vec<MetaFilter> = filters.iter().map(|text| text.parse().unwrap()).collect();
    manifest.retain(capability.as_ref(), &filters);
    manifest
        .capabilities
        .iter()
        .map(|capability| capability.name.to_string())
        .collect()
}

fn two_capabilities() -> Value {
    let due = json!({
        "name": "due", "description": "", "input_schema": true, "output_schema": true,
        "metadata": {"area": "deadlines", "level": 2}
    });
    let open = json!({
        "name": "open", "description": "", "input_schema": true, "output_schema": true
    });
    with("/capabilities", json!([due, open]))
}

#[test]
fn retaining_by_capability_keeps_only_that_capability() {
    let value = two_capabilities();
    assert_eq!(offered(&value, Some("due"), &[]), ["due"]);
    assert_eq!(offered(&value, None, &[]), ["due", "open"]);
    assert!(offered(&value, Some("late"), &[]).is_empty());
}

#[test]
fn a_capabilitys_metadata_answers_for_its_keys_and_the_plugins_for_the_rest() {
    let value = two_capabilities();
    assert_eq!(offered(&value, None, &["area=deadlines"]), ["due"]);
    assert_eq!(offered(&value, None, &["area=planning"]), ["open"]);
    assert!(offered(&value, None, &["area=planning", "area=deadlines"]).is_empty());
}

#[test]
fn a_filter_matches_only_a_string_value() {
    let value = two_capabilities();
    assert!(offered(&value, None, &["level=2"]).is_empty());
    assert_eq!(offered(&value, None, &["area="]), Vec::<String>::new());
}

#[test]
fn a_filter_is_a_non_empty_key_an_equals_sign_and_a_value() {
    assert!("area=planning".parse::<MetaFilter>().is_ok());
    assert!("area=a=b".parse::<MetaFilter>().is_ok());
    assert!("area=".parse::<MetaFilter>().is_ok());
    assert!("area".parse::<MetaFilter>().is_err());
    assert!("=planning".parse::<MetaFilter>().is_err());
}

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
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"},
            "_meta": {}
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
fn checking_the_content_alone_ignores_the_directory() {
    assert!(Manifest::from_value(manifest()).is_ok());
}

#[test]
fn a_capability_name_the_manifest_gives_must_be_a_name() {
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
    let draft7 = json!({"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"});
    assert!(matches!(
        refused(&with("/capabilities/0/outputSchema", draft7)),
        ManifestError::Schema {
            side: Side::Output,
            ..
        }
    ));
    let remote = json!({"type": "object", "$ref": "https://example.com/schema.json"});
    assert!(matches!(
        refused(&with("/capabilities/0/inputSchema", remote)),
        ManifestError::Schema {
            side: Side::Input,
            ..
        }
    ));
    let local = json!({"type": "object", "$defs": {"a": {"type": "object"}}, "$ref": "#/$defs/a"});
    assert!(parse(&with("/capabilities/0/inputSchema", local)).is_ok());
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
        "name": "due", "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"},
        "_meta": {"area": "deadlines", "level": 2}
    });
    let open = json!({
        "name": "open", "description": "", "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}
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

#[test]
fn a_schema_whose_root_is_not_an_object_type_is_refused() {
    for side in ["inputSchema", "outputSchema"] {
        let pointer = format!("/capabilities/0/{side}");
        for bad in [
            json!(true),
            json!({}),
            json!({"type": "string"}),
            json!({"type": ["object"]}),
        ] {
            assert!(
                matches!(
                    refused(&with(&pointer, bad.clone())),
                    ManifestError::NotObject { .. }
                ),
                "{side} {bad}"
            );
        }
        assert!(parse(&with(&pointer, json!({"type": "object"}))).is_ok());
    }
}

#[test]
fn a_capability_takes_mcps_title_annotations_and_meta() {
    let mut value = manifest();
    value["capabilities"][0]["title"] = json!("Due tasks");
    value["capabilities"][0]["annotations"] = json!({
        "title": "Due", "readOnlyHint": true, "destructiveHint": false,
        "idempotentHint": true, "openWorldHint": false
    });
    value["capabilities"][0]["_meta"] = json!({"area": "deadlines"});
    let capability = &parse(&value).unwrap().capabilities[0];
    assert_eq!(capability.title.as_deref(), Some("Due tasks"));
    assert_eq!(
        capability.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    assert_eq!(capability.meta["area"], "deadlines");
}

#[test]
fn an_unknown_annotation_key_is_refused() {
    let mut value = manifest();
    value["capabilities"][0]["annotations"] = json!({"readOnlyHint": true});
    assert!(parse(&value).is_ok());
    value["capabilities"][0]["annotations"]["cacheHint"] = json!(true);
    assert!(matches!(refused(&value), ManifestError::Shape { .. }));
}

#[test]
fn icons_execution_and_snake_case_schema_keys_are_refused() {
    let admitted = manifest();
    assert!(parse(&admitted).is_ok());
    for (key, field) in [
        ("icons", json!([{"src": "https://example.com/a.png"}])),
        ("execution", json!({"taskSupport": "forbidden"})),
        ("metadata", json!({})),
    ] {
        let mut value = admitted.clone();
        value["capabilities"][0][key] = field;
        assert!(
            matches!(refused(&value), ManifestError::Shape { .. }),
            "{key}"
        );
    }
    let mut value = admitted;
    let capability = value["capabilities"][0].as_object_mut().unwrap();
    let schema = capability.remove("inputSchema").unwrap();
    capability.insert("input_schema".to_owned(), schema);
    assert!(matches!(refused(&value), ManifestError::Shape { .. }));
}

#[test]
fn a_shipped_type_outside_the_plugins_namespace_is_refused() {
    let foreign = shipping("tasks", &["lr/task/v1"]);
    assert!(matches!(
        refused(&foreign),
        ManifestError::ForeignType { .. }
    ));
    let own = shipping("tasks", &["tasks/task/v1"]);
    assert_eq!(parse(&own).unwrap().types[0].as_str(), "tasks/task/v1");
}

#[test]
fn a_plugin_named_board_ships_no_board_type() {
    let error = Manifest::from_value(shipping("board", &["board/x/v1"])).unwrap_err();
    assert!(matches!(error, ManifestError::BoardType { .. }));
    assert_eq!(error.code(), Code::PluginBadManifest);
    assert!(Manifest::from_value(shipping("boards", &["boards/x/v1"])).is_ok());
}

#[test]
fn a_type_shipped_twice_is_refused() {
    let twice = shipping("tasks", &["tasks/task/v1", "tasks/task/v1"]);
    assert!(matches!(
        refused(&twice),
        ManifestError::DuplicateType { .. }
    ));
    assert_eq!(refused(&twice).code(), Code::PluginBadManifest);
    let two = shipping("tasks", &["tasks/task/v1", "tasks/task/v2"]);
    assert_eq!(parse(&two).unwrap().types.len(), 2);
}

/// The manifest renamed `name`, shipping `types`.
fn shipping(name: &str, types: &[&str]) -> Value {
    let mut manifest = with("/name", json!(name));
    manifest["types"] = json!(types);
    manifest
}

/// The manifest with its capability's `key` set to `value`.
fn capability_with(key: &str, value: Value) -> Value {
    let mut manifest = manifest();
    manifest["capabilities"][0]
        .as_object_mut()
        .unwrap()
        .insert(key.to_owned(), value);
    manifest
}

#[test]
fn a_capability_without_a_timeout_or_variables_has_neither() {
    let manifest = parse(&manifest()).unwrap();
    assert_eq!(manifest.capabilities[0].timeout, None);
    assert!(manifest.capabilities[0].env.is_empty());
}

#[test]
fn a_timeout_outside_one_second_to_an_hour_is_refused_and_one_at_either_end_admitted() {
    for seconds in [json!(0), json!(1.5)] {
        let error = refused(&capability_with("timeout", seconds.clone()));
        assert!(matches!(error, ManifestError::Shape { .. }), "{seconds}");
        assert_eq!(error.code().as_str(), "board::plugin::bad_manifest");
    }
    let over = refused(&capability_with("timeout", json!(3601)));
    assert_eq!(over.code().as_str(), "board::plugin::bad_manifest");
    assert!(
        over.to_string()
            .contains("outside 1 to 3600; set one in that range"),
        "{over}"
    );
    for seconds in [1, 3600] {
        let manifest = parse(&capability_with("timeout", json!(seconds))).unwrap();
        assert_eq!(
            manifest.capabilities[0].timeout,
            Some(std::time::Duration::from_secs(seconds))
        );
    }
}

#[test]
fn a_variable_name_that_is_not_a_shell_name_is_refused_and_one_that_is_admitted() {
    for name in ["", "1TOKEN", "A-B", "A B", "A=B"] {
        let error = refused(&capability_with("env", json!([name])));
        assert!(matches!(error, ManifestError::Shape { .. }), "{name:?}");
        assert_eq!(error.code().as_str(), "board::plugin::bad_manifest");
    }
    let manifest = parse(&capability_with("env", json!(["ANTHROPIC_API_KEY", "_x1"]))).unwrap();
    let names: Vec<&str> = manifest.capabilities[0]
        .env
        .iter()
        .map(VariableName::as_str)
        .collect();
    assert_eq!(names, ["ANTHROPIC_API_KEY", "_x1"]);
}

#[test]
fn a_variable_board_sets_is_refused_in_env_and_one_it_does_not_admitted() {
    for name in ["BOARD_DAY_START", "BOARD_PLUGIN", "PATH", "XDG_CONFIG_HOME"] {
        let error = refused(&capability_with("env", json!([name])));
        assert_eq!(error.code().as_str(), "board::plugin::bad_manifest");
        assert!(
            error
                .to_string()
                .contains("a variable Board sets for every plugin"),
            "{error}"
        );
    }
    let manifest = parse(&capability_with("env", json!(["BOARD_REVIEW_AGENT"]))).unwrap();
    assert_eq!(
        manifest.capabilities[0].env[0].as_str(),
        "BOARD_REVIEW_AGENT"
    );
}

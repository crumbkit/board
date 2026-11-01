//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates and keeps the bytes the Python plugin shipped,
//! and a capability the plugin does not offer is refused.

use std::path::Path;

use board_model::canonical_json;
use serde_json::json;

use super::*;
use crate::check::tests::recording;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/controls/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Controls.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("controls", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [ControlRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["for-path", "check", "evaluate"]);
    assert_eq!(manifest.description, skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_and_names_a_board_path() {
    let manifest = manifest();
    let [for_path, check] = [0, 1].map(|n| manifest["capabilities"][n]["inputSchema"].clone());
    for input in [&for_path, &check] {
        let properties = input["properties"].as_object().unwrap();
        assert_eq!(properties.keys().collect::<Vec<_>>(), ["path"]);
        let path = &properties["path"];
        assert!(
            path["description"].as_str().unwrap().ends_with('.'),
            "{path}"
        );
        assert_eq!(
            (&path["type"], &path["pattern"], &path["format"]),
            (&json!("string"), &json!("^/"), &json!("board-path"))
        );
        assert_eq!(input["additionalProperties"], false);
    }
    assert_eq!(for_path["required"], json!(["path"]));
    assert_eq!(check.get("required"), None);
}

#[test]
fn the_check_output_schema_admits_a_null_or_string_largest_and_refuses_a_number() {
    let manifest = Controls.manifest();
    let output = &manifest.capabilities[1].output;
    let measured = |largest: Value| {
        json!({
            "measured": [{
                "control": "/controls/c",
                "requirement": "Small.",
                "budget": "words",
                "max": 1,
                "used": 0,
                "files": [],
                "per": "file",
                "largest": largest,
                "over": [],
            }],
            "stated": [],
        })
    };
    assert!(output.validate(&measured(Value::Null)).is_ok());
    assert!(output.validate(&measured(json!("/docs/a.md"))).is_ok());
    assert!(output.validate(&measured(json!(5))).is_err());
}

/// The committed schema file holds, byte for byte, the canonical JSON of the
/// schema its struct generates. `SNAPSHOTS=overwrite` rewrites the file.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_control_schema_is_what_its_struct_generates() {
    let [(record_type, schema)] = &Controls.schemas()[..] else {
        panic!("one schema")
    };
    let file = (Path::new(env!("CARGO_MANIFEST_DIR")).join("types"))
        .join(format!("{}.schema.json", record_type.as_str()));
    let generated = canonical_json(schema);
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::write(&file, generated.as_bytes()).unwrap();
    }
    let committed = std::fs::read(&file).unwrap();
    assert!(
        committed == generated.as_bytes(),
        "{} differs from what its struct generates; run this test with SNAPSHOTS=overwrite:\n{}",
        file.display(),
        String::from_utf8_lossy(generated.as_bytes())
    );
}

#[test]
fn the_shipped_schema_refuses_a_bad_rule_or_purpose_and_admits_its_twin() {
    let [(_, schema)] = &Controls.schemas()[..] else {
        panic!("one schema")
    };
    let schema = board_validate::Schema::compile(schema.clone()).unwrap();
    let control = |fields: Value| {
        let mut record = json!({
            "$type": "controls/control/v1",
            "requirement": "R.",
            "purpose": "P.",
            "applies_to": ["/AGENTS.md"],
        });
        for (name, value) in fields.as_object().unwrap() {
            record[name] = value.clone();
        }
        record
    };
    let rule = |per: &str| json!({"rule": {"budget": "words", "max": 1, "per": per}});
    let judged = json!({"rule": {"judged": "/rubrics/plain.md"}});
    let pattern = |show: &str| json!({"rule": {"pattern": "/private/names.txt", "show": show}});
    let mixed = json!({"rule": {"budget": "words", "max": 1, "judged": "/rubrics/plain.md"}});
    let stray = json!({"rule": {"judged": "/rubrics/plain.md", "show": "pattern"}});
    for (refused, admitted) in [
        (control(rule("directory")), control(rule("file"))),
        (control(json!({"purpose": null})), control(json!({}))),
        (control(pattern("all")), control(pattern("match-only"))),
        (control(mixed), control(rule("total"))),
        (control(stray), control(judged.clone())),
        (control(json!({"rule": null})), control(judged)),
        (control(json!({"rule": {}})), control(pattern("pattern"))),
    ] {
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_check_is_answered() {
    let (_scenario, host) = recording(&[]);
    let other: CapabilityName = "budgets".parse().unwrap();
    let refused = Controls.call(&host, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "controls::call::unknown_capability");
    assert_eq!(
        refused.message,
        "controls offers no capability budgets; it offers for-path, check and evaluate"
    );
    let check: CapabilityName = "check".parse().unwrap();
    let answered = Controls.call(&host, &check, json!({})).unwrap();
    assert_eq!(answered, json!({"measured": [], "stated": []}));
}

#[test]
fn a_malformed_path_is_refused_as_board_refs_refuses_it_and_a_well_formed_one_is_answered() {
    let (_scenario, host) = recording(&[]);
    let for_path: CapabilityName = "for-path".parse().unwrap();
    let refused = (Controls.call(&host, &for_path, json!({"path": "/docs/../x"}))).unwrap_err();
    assert_eq!(refused.code, "controls::board::refused");
    assert_eq!(refused.fields["code"], json!("board::cli::usage"));
    let answered = Controls.call(&host, &for_path, json!({"path": "/docs/x"}));
    assert_eq!(answered.unwrap(), json!({"controls": []}));
}

#[test]
fn evaluate_takes_at_least_one_control_by_path_and_one_named_part_each_with_a_sentence() {
    let parsed = Controls.manifest();
    let input = &parsed.capabilities[2].input;
    let given = |controls: Value, parts: Value| json!({"controls": controls, "parts": parts});
    for (refused, admitted) in [
        (
            given(json!([]), json!({"a": "x"})),
            given(json!(["/c"]), json!({"a": "x"})),
        ),
        (
            given(json!(["c"]), json!({"a": "x"})),
            given(json!(["/c"]), json!({"a": "x"})),
        ),
        (
            given(json!(["/c"]), json!({})),
            given(json!(["/c"]), json!({"a": ""})),
        ),
        (
            given(json!(["/c"]), json!({"a": 1})),
            given(json!(["/c"]), json!({"a": "1"})),
        ),
    ] {
        assert!(input.validate(&refused).is_err(), "{refused}");
        input.validate(&admitted).unwrap();
    }
    let schema = &manifest()["capabilities"][2]["inputSchema"];
    for name in ["controls", "parts"] {
        let description = schema["properties"][name]["description"].as_str().unwrap();
        assert!(description.ends_with('.'), "{description}");
    }
}

#[test]
fn the_for_path_output_schema_admits_each_kind_of_rule_or_null_and_refuses_it_left_out() {
    let manifest = Controls.manifest();
    let output = &manifest.capabilities[0].output;
    let applying = |rule: Option<Value>| {
        let mut item = json!({"control": "/controls/c", "requirement": "R.", "purpose": "P."});
        if let Some(rule) = rule {
            item["rule"] = rule;
        }
        json!({"controls": [item]})
    };
    for rule in [
        Value::Null,
        json!({"budget": "words", "max": 1}),
        json!({"budget": "bytes", "max": 1, "per": "file"}),
        json!({"pattern": "/private/names.txt", "show": "match-only"}),
        json!({"judged": "/rubrics/plain.md"}),
    ] {
        output
            .validate(&applying(Some(rule.clone())))
            .unwrap_or_else(|e| panic!("{rule:?}: {e:?}"));
    }
    assert!(output.validate(&applying(None)).is_err());
    assert!(output.validate(&applying(Some(json!(1)))).is_err());
}

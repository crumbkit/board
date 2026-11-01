//! The manifest parses as Board reads it, each committed schema is what its
//! struct generates, and each refuses a bad record whose twin it admits.

use std::path::Path;

use board_model::{Code, canonical_json};
use board_plugin::VariableName;
use serde_json::json;

use super::*;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/boundary/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Boundary.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("boundary", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [BoundaryRecord::TYPE, CrossingRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["check", "verify", "record"]);
    let check = &manifest.capabilities[0];
    assert_eq!(
        check.timeout,
        Some(std::time::Duration::from_secs(CHECK_TIMEOUT))
    );
    let env: Vec<&str> = check.env.iter().map(VariableName::as_str).collect();
    assert_eq!(env, AGENT_VARIABLES);
    assert_eq!(manifest.description, skill_description());
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites the
/// files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Boundary.schemas() {
        let file = (Path::new(env!("CARGO_MANIFEST_DIR")).join("types"))
            .join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
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
}

/// `record` with each of `fields` set, or removed when null.
fn with(mut record: Value, fields: &Value) -> Value {
    for (name, value) in fields.as_object().unwrap() {
        if value.is_null() {
            record.as_object_mut().unwrap().remove(name);
        } else {
            record[name] = value.clone();
        }
    }
    record
}

fn compiled(record_type: &str) -> board_validate::Schema {
    let (_, schema) = (Boundary.schemas().into_iter())
        .find(|(name, _)| name.as_str() == record_type)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

#[test]
fn the_boundary_schema_refuses_a_boundary_without_checks_or_with_a_bad_field_and_admits_its_twin() {
    let schema = compiled(BoundaryRecord::TYPE);
    let check = json!({"control": "/controls/names", "parts": ["*"]});
    let boundary = json!({
        "$type": "boundary/boundary/v1",
        "to": "github:octo/*",
        "actions": ["github.*"],
        "checks": [check],
    });
    for (refused, admitted) in [
        (json!({"checks": []}), json!({})),
        (json!({"actions": []}), json!({"actions": ["*"]})),
        (
            json!({"action": ["github.*"]}),
            json!({"from": ["/repos/board"]}),
        ),
        (json!({"to": ""}), json!({"to": "*"})),
        (
            json!({"checks": [{"control": "/controls/names", "parts": []}]}),
            json!({"checks": [{"control": "/controls/names", "parts": ["pr.*"]}]}),
        ),
        (json!({"to": null}), json!({"from": null})),
    ] {
        let (refused, admitted) = (
            with(boundary.clone(), &refused),
            with(boundary.clone(), &admitted),
        );
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

#[test]
fn the_crossing_schema_refuses_a_bad_digest_null_or_unknown_field_and_admits_its_twin() {
    let schema = compiled(CrossingRecord::TYPE);
    let digest = "ab".repeat(32);
    let crossing = json!({
        "$type": "boundary/crossing/v1",
        "digest": digest,
        "action": "github.pr-create",
        "to": "github:octo/board",
        "passed": false,
        "boundaries": ["/boundaries/octo"],
        "verdicts": [{
            "control": "/controls/names",
            "part": "pr.body",
            "rule": "pattern",
            "status": "failed",
            "findings": [{"text": "acme-123", "line": 1}],
        }],
    });
    for (refused, admitted) in [
        (json!({"digest": "AB".repeat(32)}), json!({})),
        (json!({"override": "x"}), json!({"override": digest})),
        (
            json!({"result": null}),
            json!({"result": "https://example.com/pull/7"}),
        ),
        (
            json!({"from": null, "via": "/repos/board"}),
            json!({"from": "/repos/board"}),
        ),
        (json!({"passed": "no"}), json!({"passed": true})),
    ] {
        let (refused, admitted) = (
            with(crossing.clone(), &refused),
            with(crossing.clone(), &admitted),
        );
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

#[test]
fn every_input_property_carries_a_sentence() {
    let manifest = manifest();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        for (name, property) in input["properties"].as_object().unwrap() {
            let description = property["description"].as_str().unwrap();
            assert!(description.ends_with('.'), "{name}: {description}");
        }
        assert_eq!(input["additionalProperties"], false);
    }
}

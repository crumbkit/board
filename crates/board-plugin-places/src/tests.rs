//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates and refuses what it should, and a capability the
//! plugin does not offer is refused. The capabilities' own tests are in the
//! modules below; their tests through the binary are in board-cli's
//! `src/tests/built_in/places.rs`.

use std::path::Path;

use board_model::{Code, canonical_json};
use serde_json::json;

use super::*;

mod commands;
mod host;
mod listing;
mod machines;
mod workspaces;

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema the struct generates. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    for (record_type, schema) in Places.schemas() {
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

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/places/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Places.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("places", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [WorkspaceChange::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["attach", "detach", "list"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_capability_is_given_the_machine_and_mxv_variables_and_no_others() {
    for capability in super::manifest()["capabilities"].as_array().unwrap() {
        assert_eq!(
            capability["env"],
            json!(["BOARD_MACHINE", "MXV_ROOT", "XDG_STATE_HOME"]),
            "{}",
            capability["name"]
        );
    }
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let mut described = Vec::new();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        assert_eq!(input["additionalProperties"], false);
        for (name, property) in input["properties"].as_object().unwrap() {
            let sentence = property["description"].as_str().unwrap();
            assert!(sentence.ends_with('.'), "{name}: {sentence}");
            described.push(format!("{}.{name}", capability["name"].as_str().unwrap()));
        }
    }
    assert_eq!(
        described,
        [
            "attach.agent",
            "attach.change",
            "attach.person",
            "attach.program",
            "attach.session",
            "attach.workspace",
            "detach.change",
            "detach.workspace",
            "list.all",
            "list.limit",
            "list.machine",
        ]
    );
}

fn compiled() -> board_validate::Schema {
    let [(_, schema)] = <[_; 1]>::try_from(Places.schemas()).unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

/// A join as `attach` writes it, with `fields` added or replacing its own.
pub(crate) fn join(fields: &Value) -> Value {
    let mut record = json!({
        "$type": "places/workspace-change/v1",
        "workspace": "/places/quokka/fix-login",
        "change": "/changes/fix-login",
        "workspace_id": "0123456789abcdef0123456789abcdef",
        "since": "2026-11-20T09:00:00Z",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record
}

#[test]
fn the_schema_refuses_a_workspace_id_of_31_digits_and_admits_its_32_digit_twin() {
    let schema = compiled();
    let short = join(&json!({"workspace_id": "0123456789abcdef0123456789abcde"}));
    assert_eq!(
        schema.validate(&short).unwrap_err().code(),
        Code::RecordInvalid
    );
    schema.validate(&join(&json!({}))).unwrap();
}

#[test]
fn the_schema_refuses_an_unknown_field_and_admits_its_twin_with_who_wrote_it() {
    let schema = compiled();
    let unknown = join(&json!({"branch": "fix-login"}));
    assert_eq!(
        schema.validate(&unknown).unwrap_err().code(),
        Code::RecordInvalid
    );
    let by = join(&json!({"by": {"program": "claude-code", "person": "ana"}}));
    schema.validate(&by).unwrap();
}

#[test]
fn the_schema_refuses_a_time_off_utc_or_a_relative_change_and_admits_their_twins() {
    let schema = compiled();
    for (refused, code) in [
        (
            join(&json!({"since": "2026-11-20T10:00:00+01:00"})),
            Code::RecordInvalid,
        ),
        (join(&json!({"by": {}})), Code::RecordInvalid),
        (
            join(&json!({"change": "changes/fix-login"})),
            Code::RefMalformed,
        ),
    ] {
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            code,
            "{refused}"
        );
    }
    let typed = Places.schemas().remove(0).1;
    assert_eq!(
        typed["properties"]["change"]["x-board-type"],
        "changes/change/v2"
    );
    assert_eq!(typed["properties"]["workspace"].get("x-board-type"), None);
    schema
        .validate(&join(&json!({"since": "2026-11-20T09:00:00Z"})))
        .unwrap();
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_one_it_offers_runs() {
    let other: CapabilityName = "prune".parse().unwrap();
    let host = host::Real::new();
    let refused = Places.call(&host, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "places::call::unknown_capability");
    assert_eq!(
        refused.message,
        "places offers no capability prune; it offers attach, detach, list"
    );
    let known: CapabilityName = "list".parse().unwrap();
    host.mxv(&[], &[]);
    let listed = Places.call(&host, &known, json!({})).unwrap();
    assert_eq!(listed["joins"], json!([]));
}

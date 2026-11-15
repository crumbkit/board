//! The `keeper/map/v1` schema: the committed file is what the structs
//! generate, a synthetic map and an older one are admitted, each refusal has
//! an admitted twin, and the schema carries every field board top reads.

use std::path::Path;

use board_model::{Code, canonical_json};
use board_validate::Schema;

use super::*;

fn schema() -> Value {
    let [(_, schema)] = &Keeper.schemas()[..] else {
        panic!("one schema")
    };
    schema.clone()
}

/// The committed schema file holds, byte for byte, the canonical JSON of the
/// schema the structs generate. `SNAPSHOTS=overwrite` rewrites the file.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_structs_generate() {
    let generated = canonical_json(&schema());
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("types/keeper/map/v1.schema.json");
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::write(&file, generated.as_bytes()).unwrap();
    }
    let committed = std::fs::read(&file).unwrap();
    assert!(
        committed == generated.as_bytes(),
        "{} differs from what its structs generate; run this test with SNAPSHOTS=overwrite:\n{}",
        file.display(),
        String::from_utf8_lossy(generated.as_bytes())
    );
}

#[test]
fn a_synthetic_map_is_admitted_and_so_is_one_written_before_systems_and_question_rules_existed() {
    let schema = Schema::compile(schema()).unwrap();
    schema.validate(&sample_map()).unwrap();
    let mut older = sample_map();
    for field in ["systems", "question_rules"] {
        older.as_object_mut().unwrap().remove(field);
    }
    schema.validate(&older).unwrap();
}

#[test]
fn a_bad_finding_id_state_or_link_kind_is_refused_and_each_twin_is_admitted() {
    let schema = Schema::compile(schema()).unwrap();
    let with = |pointer: &str, value: Value| {
        let mut map = sample_map();
        *map.pointer_mut(pointer).unwrap() = value;
        map
    };
    for (pointer, bad, good) in [
        (
            "/findings/0/id",
            json!("stale-xyz"),
            json!("stale-0a1b2c3d"),
        ),
        (
            "/findings/0/id",
            json!("0a1b2c3d"),
            json!("stale-plan-0a1b2c3d"),
        ),
        ("/projects/0/state", json!("busy"), json!("held")),
        ("/findings/0/kind", json!("odd"), json!("loose-link")),
        (
            "/systems/0/links/0/kind",
            json!("road"),
            json!("event-stream"),
        ),
        ("/systems/0/links/0/traffic", json!(11), json!(10)),
        ("/date", json!("3 February"), json!("2026-02-04")),
    ] {
        assert_eq!(
            schema
                .validate(&with(pointer, bad.clone()))
                .unwrap_err()
                .code(),
            Code::RecordInvalid,
            "{pointer}: {bad}"
        );
        schema.validate(&with(pointer, good)).unwrap();
    }
}

#[test]
fn the_schema_carries_every_field_board_top_reads() {
    // crates/board-top/src/parse.rs reads these fields of the map.
    let schema = schema();
    let fields = |pointer: &str| -> Vec<String> {
        (schema.pointer(pointer).unwrap()["properties"]
            .as_object()
            .unwrap())
        .keys()
        .cloned()
        .collect()
    };
    let has = |names: Vec<String>, wanted: &[&str]| {
        for name in wanted {
            assert!(names.iter().any(|each| each == name), "{name} in {names:?}");
        }
    };
    has(
        fields(""),
        &["realms", "projects", "findings", "questions_waiting"],
    );
    let defs = |name: &str| format!("/$defs/{name}");
    has(fields(&defs("Realm")), &["id", "title"]);
    has(
        fields(&defs("Project")),
        &["id", "realm", "title", "state", "changes"],
    );
    has(fields(&defs("Finding")), &["kind", "paths"]);
}

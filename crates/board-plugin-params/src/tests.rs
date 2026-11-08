//! The manifest parses as Board reads it, the committed schemas are what
//! the record structs generate, the shipped heuristics are records those
//! schemas admit, and `refusal` refuses a value outside its heuristic's
//! bounds or naming another heuristic, beside the twin it admits.

use std::path::Path;

use board_model::{Code, canonical_json};
use serde::Deserialize;

use super::*;
use crate::records::Bounds;

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/params/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Params.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("params", "0.1.0")
    );
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["params/heuristic/v1", "params/value/v1"]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["get", "due"]);
    assert_eq!(manifest.description, skill_description());
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites them.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Params.schemas() {
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

/// The compiled schema of `record_type`.
fn schema(record_type: &str) -> board_validate::Schema {
    let (_, schema) = (Params.schemas().into_iter())
        .find(|(name, _)| name.as_str() == record_type)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

#[test]
fn the_shipped_heuristics_are_admitted_with_the_start_default_bounds_and_check_the_plan_set() {
    let heuristic = schema(HeuristicRecord::TYPE);
    let found: Vec<(&str, [f64; 5])> = (SHIPPED.iter())
        .map(|(name, text)| {
            let record: Value = serde_json::from_str(text).unwrap();
            heuristic.validate(&record).unwrap();
            assert_eq!(
                (&record["unit"], &record["consumer"]),
                (&json!("changed-lines"), &json!("controls")),
                "{name}"
            );
            let number = |pointer: &str| record.pointer(pointer).unwrap().as_f64().unwrap();
            let numbers = [
                "/start",
                "/default",
                "/bounds/min",
                "/bounds/max",
                "/check/fails_above",
            ];
            (*name, numbers.map(number))
        })
        .collect();
    assert_eq!(
        found,
        [
            ("overflow-agent-grace", [100.0, 200.0, 25.0, 2000.0, 0.3]),
            ("overflow-owner-grace", [400.0, 800.0, 100.0, 8000.0, 0.5]),
        ]
    );
}

/// A value of `overflow-agent-grace` that found enough data, as its schema
/// admits it.
fn value() -> Value {
    json!({
        "$type": "params/value/v1",
        "heuristic": "/params/heuristics/overflow-agent-grace",
        "value": 120,
        "scopes": {"/repos/board": 240},
        "data": "enough",
        "evidence": "/suites/params-overflow-agent-grace/runs/20261108T000000Z",
        "derived": "2026-11-08T00:00:00Z",
        "by": {"program": "claude-code"},
    })
}

#[test]
fn the_value_schema_refuses_a_bad_data_scope_or_time_and_admits_their_twins() {
    let schema = schema(ValueRecord::TYPE);
    let with = |name: &str, field: Value| {
        let mut record = value();
        record[name] = field;
        record
    };
    for (refused, admitted) in [
        (
            with("data", json!("some")),
            with("data", json!("too-little")),
        ),
        (
            with("scopes", json!({"repos/board": 1})),
            with("scopes", json!({"/repos/board": 1})),
        ),
        (
            with("scopes", json!({"/repos/board": "1"})),
            with("scopes", json!({"/repos/board": 1.5})),
        ),
        (
            with("derived", json!("2026-11-08T01:00:00+01:00")),
            with("derived", json!("2026-11-08T00:00:00Z")),
        ),
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
fn the_value_schema_finds_the_heuristic_and_the_evidence_with_their_types() {
    let schema = schema(ValueRecord::TYPE);
    let found: Vec<(String, Option<Vec<String>>)> = (schema.references(&value()).into_iter())
        .map(|reference| {
            let types =
                (reference.accepts).map(|types| types.iter().map(ToString::to_string).collect());
            (reference.pointer, types)
        })
        .collect();
    assert_eq!(
        found,
        [
            ("/evidence".to_owned(), Some(vec!["runs/run/v1".to_owned()])),
            (
                "/heuristic".to_owned(),
                Some(vec!["params/heuristic/v1".to_owned()])
            ),
        ]
    );
}

#[test]
fn the_heuristic_schema_refuses_no_inputs_and_a_recheck_of_zero_and_admits_their_twins() {
    let schema = schema(HeuristicRecord::TYPE);
    let shipped: Value = serde_json::from_str(SHIPPED[0].1).unwrap();
    let with = |name: &str, field: Value| {
        let mut record = shipped.clone();
        record[name] = field;
        record
    };
    let input = shipped["inputs"][0].clone();
    for (refused, admitted) in [
        (with("inputs", json!([])), with("inputs", json!([input]))),
        (
            with("recheck_after", json!(0)),
            with("recheck_after", json!(1)),
        ),
        (
            with("unit", json!("Changed lines")),
            with("unit", json!("changed-lines")),
        ),
    ] {
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
        schema.validate(&admitted).unwrap();
    }
}

/// `value` as the plugin reads it, with `number` as its value and
/// `heuristic` as the heuristic it names.
fn stored(number: f64, heuristic: &str) -> Stored {
    let mut record = value();
    record["value"] = json!(number);
    record["heuristic"] = json!(heuristic);
    record.as_object_mut().unwrap().remove("scopes");
    Stored::deserialize(&record).unwrap()
}

#[test]
fn a_value_outside_its_bounds_is_refused_and_one_at_the_bound_is_admitted() {
    let bounds = Bounds {
        min: 3.0,
        max: 28.0,
    };
    let own = "/params/heuristics/half-life";
    let refused = refusal("half-life", bounds, &stored(2.0, own)).unwrap();
    assert_eq!(refused.code, "params::value::out_of_bounds");
    assert_eq!(
        refused.message,
        "the value half-life is 2, outside its heuristic's bounds 3 to 28"
    );
    assert_eq!(
        (
            &refused.fields["value"],
            &refused.fields["min"],
            &refused.fields["max"]
        ),
        (&json!(2.0), &json!(3.0), &json!(28.0))
    );
    assert!(refusal("half-life", bounds, &stored(3.0, own)).is_none());
    assert!(refusal("half-life", bounds, &stored(28.0, own)).is_none());
}

#[test]
fn a_scope_outside_the_bounds_is_refused_and_its_twin_inside_them_is_admitted() {
    let bounds = Bounds {
        min: 25.0,
        max: 2000.0,
    };
    let scoped = |number: f64| {
        let mut record = value();
        record["scopes"] = json!({"/repos/board": number});
        Stored::deserialize(&record).unwrap()
    };
    let refused = refusal("overflow-agent-grace", bounds, &scoped(2001.0)).unwrap();
    assert_eq!(refused.code, "params::value::out_of_bounds");
    assert_eq!(refused.fields["scope"], json!("/repos/board"));
    assert!(refusal("overflow-agent-grace", bounds, &scoped(2000.0)).is_none());
}

#[test]
fn a_value_naming_another_heuristic_is_refused_and_its_twin_naming_its_own_is_admitted() {
    let bounds = Bounds {
        min: 3.0,
        max: 28.0,
    };
    let refused = refusal(
        "half-life",
        bounds,
        &stored(7.0, "/params/heuristics/archive-after"),
    )
    .unwrap();
    assert_eq!(refused.code, "params::value::wrong_heuristic");
    assert_eq!(
        refused.hint.as_deref(),
        Some("set heuristic to /params/heuristics/half-life")
    );
    assert!(
        refusal(
            "half-life",
            bounds,
            &stored(7.0, "/params/heuristics/half-life")
        )
        .is_none()
    );
}

#[test]
fn the_get_output_lists_its_sources_and_the_reasons_a_default_applies() {
    let output = schema_of::<Got>(SchemaSettings::draft2020_12().for_serialize());
    assert_eq!(
        output["properties"]["source"]["enum"],
        json!(["value", "default"])
    );
    assert_eq!(
        output["properties"]["reason"]["enum"],
        json!(["missing", "too-little"])
    );
}

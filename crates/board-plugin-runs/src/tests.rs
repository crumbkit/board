//! The manifest parses as Board reads it, the committed schemas are what
//! the record structs generate and keep the bytes the Python plugin
//! shipped, and a capability the plugin does not offer is refused.

use std::path::Path;

use board_model::{Code, canonical_json};
use serde_json::json;

use super::*;

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites the files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Runs.schemas() {
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

#[test]
fn the_manifest_parses_as_a_built_in_one_with_its_types_and_capabilities() {
    let manifest = Runs.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("runs", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [SuiteRecord::TYPE, RunRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["latest", "trend"]);
}

#[test]
fn every_input_property_carries_a_sentence_and_a_suite_is_a_board_path() {
    let manifest = manifest();
    let mut described = Vec::new();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        assert_eq!(input["additionalProperties"], false);
        for (name, property) in input["properties"].as_object().unwrap() {
            let sentence = property["description"].as_str().unwrap();
            assert!(sentence.ends_with('.'), "{name}: {sentence}");
            described.push(format!("{}.{name}", capability["name"].as_str().unwrap()));
        }
        let suite = &input["properties"]["suite"];
        assert_eq!(
            (&suite["type"], &suite["pattern"], &suite["format"]),
            (&json!("string"), &json!("^/"), &json!("board-path"))
        );
    }
    described.sort();
    assert_eq!(
        described,
        [
            "latest.suite",
            "trend.case",
            "trend.limit",
            "trend.measure",
            "trend.suite"
        ]
    );
}

#[test]
fn the_shipped_schemas_refuse_a_misshapen_record_and_admit_its_twin() {
    let schemas = Runs.schemas();
    let compiled = |n: usize| board_validate::Schema::compile(schemas[n].1.clone()).unwrap();
    let (suite, run) = (compiled(0), compiled(1));
    let with = |mut record: Value, fields: Value| {
        for (name, value) in fields.as_object().unwrap() {
            record[name] = value.clone();
        }
        record
    };
    let a_suite = |fields| {
        with(
            json!({"$type": "runs/suite/v1", "subjects": ["board"], "method": "Run it."}),
            fields,
        )
    };
    let a_run = |fields| {
        let record = json!({
            "$type": "runs/run/v1", "suite": "/suites/a", "by": "claude",
            "started": "2026-10-01T09:00:00Z", "scope": "full", "results": [],
        });
        with(record, fields)
    };
    let failed = |note: Option<&str>| {
        let mut result = json!({"case": "a", "outcome": "fail"});
        if let Some(note) = note {
            result["note"] = json!(note);
        }
        json!({"results": [result]})
    };
    let host =
        |cpus: u32| json!({"host": {"os": "macos", "arch": "aarch64", "logical_cpus": cpus}});
    for (schema, refused, admitted) in [
        (
            &suite,
            a_suite(json!({"cases": [{"id": "a"}], "cases_file": "/c.txt"})),
            a_suite(json!({"cases": [{"id": "a"}]})),
        ),
        (
            &suite,
            a_suite(json!({"subjects": []})),
            a_suite(json!({"subjects": ["a"]})),
        ),
        (
            &suite,
            a_suite(json!({"measures": [{"name": "ms"}]})),
            a_suite(json!({"measures": [{"name": "ms", "unit": "ms"}]})),
        ),
        (
            &suite,
            a_suite(json!({"command": null})),
            a_suite(json!({"command": "cargo test"})),
        ),
        (&run, a_run(failed(None)), a_run(failed(Some("timed out")))),
        (
            &run,
            a_run(json!({"results": [{"case": "a"}]})),
            a_run(json!({"results": [{"case": "a", "measures": {"ms": 4}}]})),
        ),
        (
            &run,
            a_run(json!({"scope": "partial"})),
            a_run(json!({"scope": "targeted"})),
        ),
        (&run, a_run(host(0)), a_run(host(1))),
        (
            &run,
            a_run(json!({"started": "2026-10-01T09:00:00"})),
            a_run(json!({})),
        ),
        (
            &run,
            a_run(json!({"written_by": "the hook"})),
            a_run(json!({"written_by": "runs/subagent-stop"})),
        ),
        (
            &run,
            a_run(json!({"measures": {"s": null}})),
            a_run(json!({"measures": {"s": true}})),
        ),
        (
            &run,
            a_run(json!({"dirty": null})),
            a_run(json!({"dirty": false})),
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
fn a_capability_the_plugin_does_not_offer_is_refused_and_latest_is_answered() {
    let (_scenario, host) =
        qualifying::tests::board(&[("suites/a.json", qualifying::tests::suite())]);
    let other: CapabilityName = "open".parse().unwrap();
    let refused = Runs.call(&host, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "runs::call::unknown_capability");
    assert_eq!(
        refused.message,
        "runs offers no capability open; it offers latest and trend"
    );
    let latest: CapabilityName = "latest".parse().unwrap();
    let answered = Runs
        .call(&host, &latest, json!({"suite": "/suites/a"}))
        .unwrap();
    assert_eq!(
        answered,
        json!({"run": null, "not_counted": 0, "unreadable": 0})
    );
}

//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates, a decision keeps references and web addresses
//! apart and needs a reason, and a capability the plugin does not offer is
//! refused.

use std::path::Path;

use board_model::{Code, canonical_json};
use board_ops::{Limit, QueryError, QueryReport, Settings};
use serde_json::json;

use super::*;

/// A host that answers every query with no rows.
struct Empty(Settings);

impl Host for Empty {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        Ok(QueryReport {
            columns: vec!["logical".into(), "code".into(), "body".into()],
            rows: Vec::new(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn settings(&self) -> &Settings {
        &self.0
    }
}

fn empty() -> Empty {
    Empty(Settings {
        now: std::time::SystemTime::UNIX_EPOCH,
        day_start: None,
        caller: None,
    })
}

fn schema() -> board_validate::Schema {
    board_validate::Schema::compile(Decisions.schemas()[0].1.clone()).unwrap()
}

fn decision(fields: &Value) -> Value {
    let mut record = json!({
        "$type": "decisions/decision/v1",
        "decision": "Use gix for ref resolution.",
        "why": "It resolved 1,000 refs in 41 ms against 63 ms for git2.",
        "decided": "2026-10-20T09:00:00Z",
        "session": "local_335a90f9",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_offers_about_and_open() {
    let manifest = Decisions.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("decisions", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [DecisionRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["about", "open"]);
}

#[test]
fn every_input_property_carries_a_sentence_and_about_needs_a_ref() {
    let manifest = manifest();
    let [about, open] = [0, 1].map(|n| manifest["capabilities"][n]["inputSchema"].clone());
    for input in [&about, &open] {
        for (name, property) in input["properties"].as_object().unwrap() {
            let description = property["description"].as_str().unwrap();
            assert!(description.ends_with('.'), "{name}: {description}");
        }
        assert_eq!(input["additionalProperties"], false);
        let limit = &input["properties"]["limit"];
        assert_eq!(
            [&limit["minimum"], &limit["maximum"], &limit["default"]],
            [&json!(1), &json!(10_000), &json!(100)]
        );
    }
    assert_eq!(about["required"], json!(["ref"]));
    assert_eq!(about["properties"]["ref"]["format"], "board-path");
    assert_eq!(open["required"], Value::Null);
    assert_eq!(open["properties"]["path"]["format"], "board-path");
}

/// The committed schema file holds, byte for byte, the canonical JSON of the
/// schema its struct generates. `SNAPSHOTS=overwrite` rewrites the file.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_file_is_what_the_record_struct_generates() {
    let [(record_type, schema)] = &Decisions.schemas()[..] else {
        panic!("one schema");
    };
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("types")
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

/// Board finds exactly the references the struct declares: any entry in
/// `basis`, and only a decision in `supersedes`.
#[test]
fn a_full_decision_validates_and_its_references_are_basis_and_supersedes() {
    let record = decision(&json!({
        "alternatives": ["Keep git2."],
        "basis": ["/evals/gix-refs", "/repos/board/docs/x.md"],
        "urls": ["https://example.com/pull/12"],
        "supersedes": ["/projects/board/decisions/old"],
    }));
    let schema = schema();
    schema.validate(&record).unwrap();
    let found: Vec<(String, Option<Vec<String>>)> = (schema.references(&record).into_iter())
        .map(|reference| {
            let accepts = reference
                .accepts
                .map(|types| types.iter().map(|kind| kind.as_str().to_owned()).collect());
            (reference.pointer, accepts)
        })
        .collect();
    assert_eq!(
        found,
        [
            ("/basis/0".to_owned(), None),
            ("/basis/1".to_owned(), None),
            (
                "/supersedes/0".to_owned(),
                Some(vec![DecisionRecord::TYPE.to_owned()])
            ),
        ]
    );
}

#[test]
fn a_decision_needs_a_reason_and_each_required_field() {
    let schema = schema();
    assert!(schema.validate(&decision(&json!({}))).is_ok());
    let refused = schema.validate(&decision(&json!({"why": ""}))).unwrap_err();
    assert_eq!(refused.code(), Code::RecordInvalid);
    for field in ["decision", "why", "decided", "session"] {
        let mut missing = decision(&json!({}));
        missing.as_object_mut().unwrap().remove(field);
        let refused = schema.validate(&missing).unwrap_err();
        assert_eq!(refused.code(), Code::RecordInvalid, "{field}");
    }
    // The fields the old plans/decision/v1 had and this one drops.
    for (field, value) in [
        ("refs", json!(["/a"])),
        ("review", json!("wanted")),
        ("written_by", json!("runs/pr-create")),
    ] {
        let refused = schema
            .validate(&decision(&json!({field: value})))
            .unwrap_err();
        assert_eq!(refused.code(), Code::RecordInvalid, "{field}");
    }
}

/// A web address belongs in `urls`, and a board path in `basis`; each is
/// refused in the other's field with its exact code.
#[test]
fn a_decision_keeps_board_paths_in_basis_and_web_addresses_in_urls() {
    let schema = schema();
    let url = "https://example.com/pull/1";
    let path = "/projects/a/answers/q";
    let code = |field: &str, value: &str| {
        (schema.validate(&decision(&json!({field: [value]}))).err()).map(|e| e.code())
    };
    assert_eq!(code("basis", url), Some(Code::RefMalformed));
    assert_eq!(code("urls", url), None);
    assert_eq!(code("urls", path), Some(Code::RecordInvalid));
    assert_eq!(code("basis", path), None);
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_open_is_answered() {
    let host = empty();
    let call = |name: &str, input| Decisions.call(&host, &name.parse().unwrap(), input);
    let refused = call("closed", json!({})).unwrap_err();
    assert_eq!(refused.code, "decisions::call::unknown_capability");
    assert_eq!(
        refused.message,
        "decisions offers no capability closed; it offers about and open"
    );
    assert_eq!(call("open", json!({})).unwrap()["total"], 0);
    let refused = call("about", json!({})).unwrap_err();
    assert_eq!(refused.code, "decisions::call::bad_input");
    assert_eq!(
        call("about", json!({"ref": "/evals/a"})).unwrap()["total"],
        0
    );
}

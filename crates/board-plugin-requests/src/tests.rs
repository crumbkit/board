//! The manifest parses as Board reads it, the schema file is what its struct
//! generates, and a capability the plugin does not offer is refused.

use std::path::Path;

use board_model::canonical_json;
use board_ops::{Limit, QueryError, QueryReport, Settings};
use serde_json::json;

use super::*;

/// A host that answers every query with no rows.
struct Empty(Settings);

impl Host for Empty {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        Ok(QueryReport {
            columns: vec!["logical".into(), "body".into()],
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

#[test]
fn the_manifest_parses_as_board_reads_a_built_in_one_and_offers_thread() {
    let manifest = Requests.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("requests", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["requests/reply/v1"]);
    let [thread] = &manifest.capabilities[..] else {
        panic!("{:?}", manifest.capabilities)
    };
    assert_eq!(thread.name.as_str(), "thread");
}

/// The committed schema file of each record type the plugin ships is
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites the files.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_reply_schema_is_what_its_struct_generates() {
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    let overwrite = std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite");
    let shipped = Requests.schemas();
    assert_eq!(shipped.len(), 1);
    for (record_type, schema) in shipped {
        let file = types.join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if overwrite {
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
fn a_reply_without_text_or_naming_a_malformed_effect_is_refused_and_its_twin_is_admitted() {
    let schema = board_validate::Schema::compile(reply_schema()).unwrap();
    let reply = json!({"$type": "requests/reply/v1", "effect": "a".repeat(64), "text": "Why?"});
    schema.validate(&reply).unwrap();
    for (key, value) in [("text", json!("")), ("effect", json!("A".repeat(64)))] {
        let mut refused = reply.clone();
        refused[key] = value;
        assert!(schema.validate(&refused).is_err(), "{key}");
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_thread_reaches_its_own_check() {
    let other: CapabilityName = "other".parse().unwrap();
    let refused = Requests.call(&empty(), &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "requests::call::unknown_capability");
    let thread: CapabilityName = "thread".parse().unwrap();
    let answered = Requests.call(&empty(), &thread, json!({"effect": "a".repeat(64)}));
    assert_eq!(answered.unwrap_err().code, "requests::effect::not_found");
}

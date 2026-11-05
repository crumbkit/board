//! The manifest parses as Board reads it, the schema file is what its struct
//! generates and admits a topic and refuses its malformed twins, and a call
//! is refused since the plugin offers no capability.

use std::path::Path;

use board_model::canonical_json;
use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::*;

#[test]
fn the_manifest_parses_and_names_the_one_type_and_no_capability() {
    let manifest = Topics.manifest();
    assert_eq!(manifest.name.as_str(), "topics");
    assert!(manifest.capabilities.is_empty());
    assert_eq!(
        (Topics.schemas().into_iter())
            .map(|(name, _)| name.as_str().to_owned())
            .collect::<Vec<_>>(),
        ["topics/topic/v1"]
    );
}

/// The committed schema file is the schema the struct generates.
/// `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_topic_schema_is_what_its_struct_generates() {
    let types = Path::new(env!("CARGO_MANIFEST_DIR")).join("types");
    let overwrite = std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite");
    for (record_type, schema) in Topics.schemas() {
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

/// A topic needs a title, its links each take their shape, and a list is
/// left out rather than empty.
#[test]
fn a_topic_with_a_malformed_field_is_refused_and_its_twin_is_admitted() {
    let schema = board_validate::Schema::compile(topic_schema()).unwrap();
    let topic = json!({
        "$type": "topics/topic/v1",
        "title": "seatbelt",
        "description": "The macOS sandbox that sandbox-exec runs a process in.",
        "aliases": ["sandbox-exec"],
        "urls": ["https://developer.apple.com/documentation/security/app-sandbox"],
        "repos": ["crumbkit/typescript-standards", "cogentforge/typescript-standards"],
        "refs": ["/evals/agent-sandbox-macos"],
    });
    schema.validate(&topic).unwrap();
    schema
        .validate(&json!({"$type": "topics/topic/v1", "title": "uv"}))
        .unwrap();
    for (key, value) in [
        ("title", json!("")),
        ("aliases", json!([])),
        ("urls", json!(["developer.apple.com"])),
        ("repos", json!(["typescript-standards"])),
        ("refs", json!(["evals/agent-sandbox-macos"])),
        ("colour", json!("blue")),
    ] {
        let mut refused = topic.clone();
        refused[key] = value;
        assert!(schema.validate(&refused).is_err(), "{key}");
    }
    let mut untitled = topic;
    untitled.as_object_mut().unwrap().remove("title");
    assert!(schema.validate(&untitled).is_err());
}

/// Board refuses a call to a capability the manifest does not list before
/// the plugin runs, so a call to topics never reaches it.
#[test]
fn a_call_is_refused_because_the_plugin_offers_no_capability() {
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.build_in(&[&Topics]);
    board
        .plugin_enable(None, None, &"topics".parse().unwrap(), &[])
        .unwrap();
    let target: Target = "topics.find".parse().unwrap();
    let called = board.call(&target, b"{}", &Plugins::default());
    let code = called
        .map(|_| ())
        .map_err(|failed| failed.refusal().code.as_str().to_owned());
    assert_eq!(code, Err("board::plugin::not_installed".to_owned()));
}

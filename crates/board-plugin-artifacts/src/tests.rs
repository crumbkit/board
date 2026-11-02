//! The manifest parses as Board reads it, the committed schemas are what the
//! record structs generate, each schema refuses a bad record beside its
//! admitted twin, and a capability the plugin does not offer is refused.

use std::path::Path;

use board_model::canonical_json;
use serde_json::json;

use super::*;

/// A host that does nothing; each call here is refused before the plugin
/// reaches the board.
struct Nothing;

impl Host for Nothing {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("a refused call runs no query")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("a refused call resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("a refused call reads no record")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("a refused call writes no record")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("artifacts calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("artifacts opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("a refused call asks the owner nothing")
    }

    fn settings(&self) -> &board_ops::Settings {
        panic!("a refused call reads no settings")
    }
}

/// Each committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates. `SNAPSHOTS=overwrite` rewrites them.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schemas_are_what_their_structs_generate() {
    for (record_type, schema) in Artifacts.schemas() {
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
    let skill = include_str!("../../../plugins/artifacts/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Artifacts.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("artifacts", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [ArtifactRecord::TYPE, VersionRecord::TYPE]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["list", "import", "record", "set", "agree"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    for capability in super::manifest()["capabilities"].as_array().unwrap() {
        let input = &capability["inputSchema"];
        assert_eq!(
            input["additionalProperties"], false,
            "{}",
            capability["name"]
        );
        for (name, property) in input["properties"].as_object().unwrap() {
            let sentence = property["description"].as_str().unwrap();
            assert!(sentence.ends_with('.'), "{name}: {sentence}");
        }
    }
}

/// The compiled schema of `kind`.
fn schema(kind: &str) -> board_validate::Schema {
    let (_, schema) = (Artifacts.schemas().into_iter())
        .find(|(name, _)| name.as_str() == kind)
        .unwrap();
    board_validate::Schema::compile(schema).unwrap()
}

fn artifact(fields: &Value) -> Value {
    let mut record = json!({
        "$type": "artifacts/artifact/v1",
        "url": "https://claude.ai/artifact/AbCdEf0123456789xyzQ",
        "account": "owner@example.com",
        "first_seen": "2026-11-01T09:00:00Z",
        "state": "draft",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record
}

#[test]
fn an_agreed_artifact_must_name_its_decision_and_a_draft_one_need_not() {
    let schema = schema(ArtifactRecord::TYPE);
    assert!(schema.validate(&artifact(&json!({}))).is_ok());
    assert!(
        schema
            .validate(&artifact(&json!({"state": "agreed"})))
            .is_err()
    );
    let agreed = json!({"state": "agreed", "agreed_by": "/projects/a/decisions/d"});
    assert!(schema.validate(&artifact(&agreed)).is_ok());
}

#[test]
fn an_artifact_takes_area_tags_of_lowercase_parts_and_refuses_others() {
    let schema = schema(ArtifactRecord::TYPE);
    for (area, admitted) in [
        ("dashboard/flows", true),
        ("ios", true),
        ("call-flows/step-list", true),
        ("Dashboard/flows", false),
        ("dashboard/", false),
        ("dash board", false),
        ("-flows", false),
    ] {
        let record = artifact(&json!({"area": [area]}));
        assert_eq!(schema.validate(&record).is_ok(), admitted, "{area}");
    }
}

#[test]
fn an_artifact_refuses_an_unknown_state_or_field_and_admits_each_known_state() {
    let schema = schema(ArtifactRecord::TYPE);
    for state in ["draft", "shown", "superseded"] {
        assert!(
            schema.validate(&artifact(&json!({"state": state}))).is_ok(),
            "{state}"
        );
    }
    assert!(
        schema
            .validate(&artifact(&json!({"state": "approved"})))
            .is_err()
    );
    assert!(
        schema
            .validate(&artifact(&json!({"for": "/changes/a"})))
            .is_err()
    );
    assert!(schema.validate(&artifact(&json!({"shows": ""}))).is_err());
    assert!(
        schema
            .validate(&artifact(&json!({"shows": "The step list."})))
            .is_ok()
    );
}

fn version(fields: &Value) -> Value {
    let mut record = json!({
        "$type": "artifacts/version/v1",
        "version": "1793473476-1c42",
        "session": "session-1",
        "at": "2026-11-01T09:00:00Z",
    });
    for (name, value) in fields.as_object().unwrap() {
        record[name] = value.clone();
    }
    record
}

#[test]
fn a_version_has_either_a_copy_with_its_hash_or_none_and_no_hash() {
    let schema = schema(VersionRecord::TYPE);
    let sha = "ab".repeat(32);
    assert!(schema.validate(&version(&json!({"sha256": sha}))).is_ok());
    assert!(schema.validate(&version(&json!({"copy": "none"}))).is_ok());
    assert!(schema.validate(&version(&json!({}))).is_err());
    assert!(
        schema
            .validate(&version(&json!({"copy": "none", "sha256": sha})))
            .is_err()
    );
    assert!(schema.validate(&version(&json!({"copy": "all"}))).is_err());
}

/// The brief line's pattern and parser in `text`: the `BRIEF` line, and
/// `briefOf` from its doc comment to its closing brace.
fn brief_parser(text: &str) -> String {
    let pattern = (text.lines())
        .find(|line| line.starts_with("const BRIEF = "))
        .unwrap();
    let start = text
        .find("/** The change the first line of `text` names")
        .unwrap();
    let length = text[start..].find("\n}\n").unwrap() + 2;
    format!("{pattern}\n{}", &text[start..start + length])
}

#[test]
fn the_artifacts_mod_reads_the_brief_line_with_the_write_guards_parser() {
    let guard = include_str!("../../../plugins/changes/claude-code/hooks/guard.ts");
    let artifacts = include_str!("../../../plugins/artifacts/claude-code/hooks/brief.ts");
    assert_eq!(brief_parser(artifacts), brief_parser(guard));
    assert!(brief_parser(guard).contains("export function briefOf"));
}

#[test]
fn an_artifact_is_a_claude_ai_link_an_account_and_a_time_with_its_zone() {
    let schema = schema(ArtifactRecord::TYPE);
    for (fields, admitted) in [
        (json!({}), true),
        (
            json!({"url": "https://claude.ai/code/artifact/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21"}),
            true,
        ),
        (json!({"url": "https://example.com/artifact/AbCd"}), false),
        (json!({"url": "https://claude.ai/artifact/../AbCd"}), false),
        (json!({"url": "https://example.com/../elsewhere"}), false),
        (json!({"account": ""}), false),
        (json!({"first_seen": "2026-10-31T18:20:00"}), false),
    ] {
        let record = artifact(&fields);
        assert_eq!(schema.validate(&record).is_ok(), admitted, "{fields}");
    }
}

#[test]
fn a_version_names_its_session_and_time_and_may_leave_its_version_id_out() {
    let schema = schema(VersionRecord::TYPE);
    let copied = json!({"sha256": "ab".repeat(32)});
    assert!(schema.validate(&version(&copied)).is_ok());
    let mut keyed = version(&copied);
    keyed.as_object_mut().unwrap().remove("version");
    assert!(schema.validate(&keyed).is_ok());
    for fields in [
        json!({"session": ""}),
        json!({"at": "2026-10-31T18:20:00"}),
        json!({"version": "1/2"}),
        json!({"pinned": true}),
    ] {
        let mut record = version(&copied);
        for (name, value) in fields.as_object().unwrap() {
            record[name] = value.clone();
        }
        assert!(schema.validate(&record).is_err(), "{fields}");
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_set_reaches_its_own_check() {
    let other: CapabilityName = "publish".parse().unwrap();
    let refused = Artifacts.call(&Nothing, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "artifacts::call::unknown_capability");
    assert_eq!(
        refused.message,
        "artifacts offers no capability publish; it offers list, import, record, set and agree"
    );
    let set: CapabilityName = "set".parse().unwrap();
    let refused = (Artifacts.call(&Nothing, &set, json!({"path": "/artifacts/a"}))).unwrap_err();
    assert_eq!(refused.code, "artifacts::set::nothing");
    assert_eq!(refused.fields["path"], "/artifacts/a");
}

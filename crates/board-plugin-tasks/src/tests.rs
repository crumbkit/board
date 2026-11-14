//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates, every input says what it means, every output
//! field is required, and a capability the plugin does not offer is
//! refused. These were the plugin's `test_manifest.py`; what `board check`
//! admits in a task is the parity suite's, in board-cli.

use std::path::Path;
use std::time::Duration;

use board_model::{LogicalPath, Refusal, canonical_json};
use board_ops::Elsewhere;
use serde_json::json;

use super::*;

pub(crate) mod real;

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema its struct generates, which is the schema the Python plugin
/// shipped. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema files are goldens, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    for (record_type, schema) in Tasks.schemas() {
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
    let skill = include_str!("../../../plugins/tasks/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Tasks.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("tasks", "0.4.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["tasks/task/v2"]);
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "available",
            "tree",
            "add",
            "claim",
            "done",
            "defer",
            "drop",
            "reviewed",
            "move"
        ]
    );
    assert_eq!(super::manifest()["description"], skill_description());
}

#[test]
fn every_write_but_done_and_move_says_it_syncs_pushes_and_sets_reviewed() {
    let manifest = super::manifest();
    for capability in manifest["capabilities"].as_array().unwrap() {
        let name = capability["name"].as_str().unwrap();
        let description = capability["description"].as_str().unwrap();
        let stamps = description.ends_with(WRITES);
        assert_eq!(
            stamps,
            !["available", "tree", "done", "move"].contains(&name),
            "{name}"
        );
        if name == "done" {
            assert!(
                description.ends_with(
                    " It syncs the task's repo first and pushes after, unless --push false makes \
                     it commit in the clone alone, and sets reviewed."
                ),
                "{description}"
            );
        }
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
    described.sort();
    let mut expected = vec![
        "available.agent",
        "available.context",
        "available.owner",
        "available.person",
        "available.program",
        "available.remote",
        "available.repo",
        "available.session",
        "tree.path",
        "tree.remote",
        "tree.repo",
        "tree.root",
        "add.after",
        "add.brief",
        "add.contexts",
        "add.defer",
        "add.due",
        "add.ephemeral",
        "add.from_hour",
        "add.kind",
        "add.parent",
        "add.remote",
        "add.repeat",
        "add.repo",
        "add.text",
        "add.title",
        "claim.agent",
        "claim.owner",
        "claim.path",
        "claim.person",
        "claim.program",
        "claim.session",
        "done.path",
        "done.push",
        "defer.path",
        "defer.until",
        "drop.path",
        "reviewed.path",
        "move.parent",
        "move.path",
    ];
    expected.sort_unstable();
    assert_eq!(described, expected);
}

#[test]
fn every_output_property_is_required_so_a_null_is_never_left_out() {
    let manifest = super::manifest();
    let required = |name: &str| {
        let capabilities = manifest["capabilities"].as_array().unwrap();
        let capability = capabilities
            .iter()
            .find(|each| each["name"] == name)
            .unwrap();
        capability["outputSchema"]["required"].clone()
    };
    assert_eq!(
        required("available"),
        json!([
            "available",
            "due_soon",
            "stale",
            "held",
            "repos",
            "degraded",
            "unsynced",
            "cross_repo",
            "origin_unread"
        ])
    );
    assert_eq!(
        required("tree"),
        json!(["tree", "roots", "total", "repos", "cycles", "cross_repo"])
    );
    let written = json!([
        "repo",
        "path",
        "status",
        "defer",
        "owner",
        "reviewed",
        "commit",
        "unsynced",
        "unpushed",
        "conflicts"
    ]);
    for name in ["add", "claim", "done", "defer", "drop", "reviewed"] {
        assert_eq!(required(name), written, "{name}");
    }
    assert_eq!(
        required("move"),
        json!([
            "repo",
            "path",
            "parent",
            "commit",
            "unsynced",
            "unpushed",
            "conflicts"
        ])
    );
}

/// A host that reaches nothing of the board: the unknown capability is
/// refused before any read.
struct Nowhere;

impl Host for Nowhere {
    fn query(
        &self,
        _: &str,
        _: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("an unknown capability queries nothing")
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("an unknown capability resolves no refs")
    }
    fn get_each(
        &self,
        _: &[LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("an unknown capability reads nothing")
    }
    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("an unknown capability writes nothing")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("tasks calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("an unknown capability opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<Refusal>> {
        panic!("tasks asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        panic!("an unknown capability syncs nothing")
    }
    fn mount_update(
        &self,
        _: &LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        panic!("an unknown capability updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("an unknown capability reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_naming_what_it_offers() {
    let capability: CapabilityName = "find".parse().unwrap();
    let refused = Tasks.call(&Nowhere, &capability, json!({})).unwrap_err();
    assert_eq!(refused.code, "tasks::call::unknown_capability");
    assert_eq!(
        refused.message,
        "tasks offers no capability find; it offers available, tree, add, claim, done, defer, \
         drop, reviewed and move"
    );
}

#[test]
fn a_capability_the_plugin_offers_is_not_refused_as_unknown() {
    let capability: CapabilityName = "tree".parse().unwrap();
    let refused = Tasks.call(
        &Nowhere,
        &capability,
        json!({"root": true, "path": "/tasks/x.md"}),
    );
    assert_eq!(refused.unwrap_err().code, "tasks::tree::path_or_root");
}

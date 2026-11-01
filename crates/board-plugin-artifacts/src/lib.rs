//! The artifacts plugin built into Board (plugins/artifacts/README.md).

mod agree;
mod held;
mod import;
mod list;
mod record;
pub mod records;
mod set;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{ArtifactRecord, VersionRecord};

#[derive(Clone, Copy, Debug)]
pub struct Artifacts;

impl Plugin for Artifacts {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        record_schemas()
            .into_iter()
            .map(|(name, schema)| {
                let name = name
                    .parse()
                    .expect("each record's TYPE is a record type; a test parses it");
                (name, schema)
            })
            .collect()
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "list" => list::list(host, &input),
            "import" => import::import(host, &input),
            "record" => record::record(host, &input),
            "set" => set::set(host, &input),
            "agree" => agree::agree(host, &input),
            other => Err(PluginError {
                code: "artifacts::call::unknown_capability".to_owned(),
                message: format!(
                    "artifacts offers no capability {other}; it offers list, import, record, set \
                     and agree"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

/// One capability's manifest entry.
fn capability<I: JsonSchema, O: JsonSchema>(
    name: &str,
    description: &str,
    read_only: bool,
) -> Value {
    let mut entry = json!({
        "name": name,
        "description": description,
        "inputSchema": schema_of::<I>(SchemaSettings::draft2020_12()),
        "outputSchema": schema_of::<O>(SchemaSettings::draft2020_12().for_serialize()),
        "_meta": {},
    });
    if read_only {
        entry["annotations"] = json!({"readOnlyHint": true});
    }
    entry
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "artifacts",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "The claude.ai artifacts the owner reviews, each with the task or change it \
            belongs to, what it shows, whether the owner agreed it, and a copy of every version's \
            page, recorded by the board-artifacts Claude Code mod. Use it to find an artifact, to \
            file one, or to sync the artifacts published outside Claude Code. Start with `board \
            --personal call artifacts.list --subject /changes/<name>`, which lists that work's \
            artifacts with every version, newest first. SKILL.md and README.md in the plugin's \
            directory say more.",
        "metadata": {"area": "publishing"},
        "types": record_schemas().map(|(name, _)| name),
        "capabilities": [
            capability::<list::Input, list::Output>(
                "list",
                "The artifacts in the board with every version, newest first, by account, \
                 subject, state, area, or with no subject or shows.",
                true,
            ),
            capability::<import::Input, import::Output>(
                "import",
                "Write an artifact record, without a version, for each artifact of an Artifact \
                 tool listing that the board does not have.",
                false,
            ),
            capability::<record::Input, record::Output>(
                "record",
                "Write the artifact record a session published or read when it is missing, with \
                 the session's subject, and name the shown or agreed artifacts it may replace.",
                false,
            ),
            capability::<set::Input, set::Output>(
                "set",
                "Add a subject or area to an artifact, set what it shows or whether it was \
                 shown, and mark the artifact it replaces superseded.",
                false,
            ),
            capability::<agree::Input, agree::Output>(
                "agree",
                "Move an artifact to agreed, naming the owner's decision that accepts it.",
                false,
            ),
        ],
    })
}

/// Each record type the plugin ships, with the schema its struct generates.
fn record_schemas() -> [(&'static str, Value); 2] {
    let settings = SchemaSettings::draft2020_12;
    [
        (
            ArtifactRecord::TYPE,
            schema_of::<ArtifactRecord>(settings()),
        ),
        (VersionRecord::TYPE, schema_of::<VersionRecord>(settings())),
    ]
}

#[cfg(test)]
mod tests;

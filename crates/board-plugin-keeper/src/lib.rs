//! The keeper plugin built into Board (plugins/keeper/README.md).

mod gather;
mod host;
mod map;
mod model;
mod placement;
pub mod records;
mod render;
mod stale;
mod survey;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use records::KeeperMap;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

/// The plugin's name, which prefixes its error codes.
pub(crate) const PLUGIN: &str = "keeper";

/// The latest map, which `map` writes beside its timestamped record.
pub(crate) const LATEST: &str = "/projects/keeper/map/latest";

/// A refusal with `code`, `message` and the named `fields`.
pub(crate) fn refusal<const N: usize>(
    code: &str,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        hint: None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Keeper;

impl Plugin for Keeper {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name =
            (KeeperMap::TYPE.parse()).expect("KeeperMap's TYPE is a record type; a test parses it");
        vec![(name, schema_of::<KeeperMap>(SchemaSettings::draft2020_12()))]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "gather" => gather::gather(host, &input),
            "map" => map::map(host, &input),
            "render" => render::render(host, &input),
            other => Err(PluginError {
                code: "keeper::call::unknown_capability".to_owned(),
                message: format!(
                    "keeper offers no capability {other}; it offers gather, map and render"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn read_only() -> Value {
    json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    })
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "keeper",
        "version": "0.1.0",
        "description": skill_description(),
        "metadata": {"area": "work"},
        "types": [KeeperMap::TYPE],
        "capabilities": [{
            "name": "gather",
            "description": "The live facts a consolidation pass needs.",
            "inputSchema": schema_of::<gather::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": {"type": "object"},
            "annotations": read_only(),
            "_meta": {},
        }, {
            "name": "map",
            "description": "Checks a pass's project tree and writes it as a keeper map.",
            "inputSchema": schema_of::<map::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": {"type": "object"},
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false,
            },
            "_meta": {},
        }, {
            "name": "render",
            "description": "Writes the status board page.",
            "inputSchema": schema_of::<render::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": {"type": "object"},
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }],
    })
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML scalar
/// on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/keeper/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .expect("SKILL.md's frontmatter has a description");
    let quoted = (line
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\'')))
    .expect("the description is a single-quoted scalar");
    quoted.replace("''", "'")
}

#[cfg(test)]
mod tests;

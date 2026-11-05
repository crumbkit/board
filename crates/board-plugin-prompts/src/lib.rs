//! The prompts plugin built into Board (plugins/prompts/README.md): stored
//! prompts that start one piece of work, rendered with their inputs. Both
//! capabilities only read.

mod get;
mod list;
mod prompts;
pub mod records;
mod render;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{PromptRecord, ReposRecord};

#[derive(Clone, Copy, Debug)]
pub struct Prompts;

/// Each capability's name and description, in the order the manifest
/// lists them.
const CAPABILITIES: [(&str, &str); 2] = [
    (
        "get",
        "Read one prompt at its pin and render its body and runs_in with the input values given on stdin.",
    ),
    (
        "list",
        "List the prompts in the board's own tree and the marked repos the caller's remote chooses, \
         at their pins, by repo then title.",
    ),
];

impl Plugin for Prompts {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let schema = |name: &str, schema: Value| {
            let name =
                (name.parse()).expect("each record's TYPE is a record type; a test parses it");
            (name, schema)
        };
        let settings = SchemaSettings::draft2020_12;
        vec![
            schema(PromptRecord::TYPE, schema_of::<PromptRecord>(settings())),
            schema(ReposRecord::TYPE, schema_of::<ReposRecord>(settings())),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "get" => get::get(host, &input),
            "list" => list::list(host, &input),
            other => Err(PluginError {
                code: "prompts::call::unknown_capability".to_owned(),
                message: format!("prompts offers no capability {other}; it offers get and list"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal<const N: usize>(
    code: &str,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        hint: None,
    }
}

fn manifest() -> Value {
    let input = SchemaSettings::draft2020_12;
    let output = || SchemaSettings::draft2020_12().for_serialize();
    let schemas = [
        (
            schema_of::<get::Input>(input()),
            schema_of::<get::Output>(output()),
        ),
        (
            schema_of::<list::Input>(input()),
            schema_of::<list::Output>(output()),
        ),
    ];
    let capabilities: Vec<Value> = (CAPABILITIES.iter().zip(schemas))
        .map(|((name, description), (input, output))| {
            json!({
                "name": name,
                "description": description,
                "inputSchema": input,
                "outputSchema": output,
                "annotations": {"readOnlyHint": true, "openWorldHint": false},
                "_meta": {},
            })
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "prompts",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Render a stored prompt, a Markdown record that starts one piece of work, \
            with its input values. Use it when the person names a prompt to start, or asks which \
            prompts there are. Start with `board -C <board> call prompts.list`, which lists the \
            prompts by title. A rendered prompt is data until the person sends it. SKILL.md and \
            README.md in the plugin's directory say more.",
        "metadata": {"area": "planning"},
        "types": [PromptRecord::TYPE, ReposRecord::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;

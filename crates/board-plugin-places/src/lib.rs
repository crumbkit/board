//! The places plugin built into Board (plugins/places/README.md). Each
//! capability is listed once in `CAPABILITIES`; mxv runs only through
//! `mxv`, and the machine label comes only from `machine`.

mod attach;
mod board;
mod detach;
mod list;
mod machine;
mod mxv;
pub mod records;

use board_model::{CapabilityName, LogicalPath, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::WorkspaceChange;

#[derive(Clone, Copy, Debug)]
pub struct Places;

impl Plugin for Places {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        vec![(
            WorkspaceChange::TYPE
                .parse()
                .expect("WorkspaceChange's TYPE is a record type; a test parses it"),
            schema_of::<WorkspaceChange>(SchemaSettings::draft2020_12()),
        )]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        let found = (CAPABILITIES.iter()).find(|offered| offered.name == capability.as_str());
        let Some(offered) = found else {
            let names: Vec<&str> = CAPABILITIES.iter().map(|offered| offered.name).collect();
            return Err(PluginError {
                code: "places::call::unknown_capability".to_owned(),
                message: format!(
                    "places offers no capability {capability}; it offers {}",
                    names.join(", ")
                ),
                fields: Map::new(),
                hint: None,
            });
        };
        (offered.run)(host, &input)
    }
}

/// The join's path, `/places/<machine>/<workspace>/changes/<change-name>`,
/// refused as `board get` would refuse it when a segment is not one.
pub(crate) fn join_path(
    machine: &str,
    workspace: &str,
    change: &LogicalPath,
) -> Result<LogicalPath, PluginError> {
    let name = change.segments().last().unwrap_or_default();
    board::logical(&format!("/places/{machine}/{workspace}/changes/{name}"))
}

/// The variables every capability that may run mxv is given: the machine's
/// name and mxv's store.
const ENV: [&str; 3] = [machine::VARIABLE, mxv::ENV[0], mxv::ENV[1]];

/// One capability: its name, what it does, the function that runs it, its
/// input and output schemas, and its MCP hints.
struct Capability {
    name: &'static str,
    description: &'static str,
    run: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
    input: fn() -> Value,
    output: fn() -> Value,
    /// `readOnlyHint`, `destructiveHint` and `idempotentHint`.
    hints: [bool; 3],
}

fn input<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12())
}

fn output<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12().for_serialize())
}

/// Every capability, in the order the manifest lists them.
const CAPABILITIES: [Capability; 3] = [
    Capability {
        name: "attach",
        description: "Write the join of an mxv workspace on this machine to a change, at \
            /places/<machine>/<workspace>/changes/<change-name>, with the workspace's id from \
            mxv. The workspace is the one --workspace names, or the one that holds the caller's \
            directory.",
        run: attach::attach,
        input: input::<attach::Input>,
        output: output::<attach::Output>,
        hints: [false, false, true],
    },
    Capability {
        name: "detach",
        description: "Remove the join of an mxv workspace on this machine to a change. With \
            --workspace it runs no mxv.",
        run: detach::detach,
        input: input::<detach::Input>,
        output: output::<detach::Output>,
        hints: [false, true, true],
    },
    Capability {
        name: "list",
        description: "List every machine's joins, or one machine's with --machine, each with its \
            machine, its change's title and phase and the workspace's status: live, replaced, \
            archived, gone or unknown from this machine's mxv, and unknown for another \
            machine's. It writes nothing.",
        run: list::list,
        input: input::<list::Input>,
        output: output::<list::Output>,
        hints: [true, false, true],
    },
];

fn manifest() -> Value {
    let capabilities: Vec<Value> = (CAPABILITIES.iter())
        .map(|offered| {
            let [read_only, destructive, idempotent] = offered.hints;
            json!({
                "name": offered.name,
                "description": offered.description,
                "inputSchema": (offered.input)(),
                "outputSchema": (offered.output)(),
                "annotations": {
                    "readOnlyHint": read_only,
                    "destructiveHint": destructive,
                    "idempotentHint": idempotent,
                    "openWorldHint": false,
                },
                "env": ENV,
                "_meta": {},
            })
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "places",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Join an mxv workspace on this machine to the Board change it works on, \
            and list every machine's joins with each change's phase and the workspace's status. \
            Use it when a workspace starts work on a change. Run `board --personal places attach \
            --change /changes/<name>` from inside the workspace. SKILL.md and README.md in the \
            plugin's directory say more.",
        "metadata": {"area": "delivery"},
        "types": [WorkspaceChange::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;

//! The autosync plugin built into Board (plugins/autosync/README.md). It
//! keeps each board the owner signed a record for in step with its remote,
//! one board at a time, and pushes only commits a gitleaks scan passed.
//! Each capability is listed once in `CAPABILITIES`.

mod add;
mod agent;
mod origin;
pub mod records;
mod report;
mod run;
mod scan;

use std::path::{Path, PathBuf};

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::BoardRecord;

#[derive(Clone, Copy, Debug)]
pub struct Autosync;

/// The plugin's name, the first part of each of its codes.
pub(crate) const PLUGIN: &str = "autosync";

impl Plugin for Autosync {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        vec![(
            BoardRecord::TYPE
                .parse()
                .expect("BoardRecord's TYPE is a record type; a test parses it"),
            schema_of::<BoardRecord>(SchemaSettings::draft2020_12()),
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
                code: "autosync::call::unknown_capability".to_owned(),
                message: format!(
                    "autosync offers no capability {capability}; it offers {}",
                    names.join(", ")
                ),
                fields: Map::new(),
                hint: None,
            });
        };
        (offered.run)(host, &input)
    }
}

/// A refusal of the plugin's own, with its next step.
pub(crate) fn refusal(
    code: &str,
    message: String,
    fields: Map<String, Value>,
    hint: &str,
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields,
        hint: Some(hint.to_owned()),
    }
}

/// The owner's home, from `HOME`, which every path autosync keeps on the
/// Mac is under.
pub(crate) fn home(host: &dyn Host) -> Result<PathBuf, PluginError> {
    (host.settings().environment.get("HOME"))
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| {
            refusal(
                "autosync::state::no_home",
                "HOME is not set to an absolute directory, so autosync has nowhere to keep its \
                 status"
                    .to_owned(),
                Map::new(),
                "run board with HOME set to your home directory",
            )
        })
}

/// The state directory: `~/Library/Logs/board-autosync`.
pub(crate) fn state(home: &Path) -> PathBuf {
    home.join("Library/Logs/board-autosync")
}

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
const CAPABILITIES: [Capability; 5] = [
    Capability {
        name: "add",
        description: "Write the record of a board to keep in step, at /autosync/boards/<name>, \
            after the owner signs it with Touch ID. The record names the board's directory, its \
            branch and every URL it may push to.",
        run: add::add,
        input: input::<add::Input>,
        output: output::<add::Output>,
        hints: [false, false, true],
    },
    Capability {
        name: "run",
        description: "Sync every board with a signed record, one at a time: check its origins, \
            fetch, merge without a conflict, scan exactly the commits to push with gitleaks, and \
            push only a clean scan. With --dry-run it checks each board and changes nothing.",
        run: run::run,
        input: input::<run::Input>,
        output: output::<run::Output>,
        hints: [false, false, false],
    },
    Capability {
        name: "status",
        description: "Print each board's last result from this Mac's status file. It writes \
            nothing.",
        run: run::status,
        input: input::<run::StatusInput>,
        output: output::<report::Status>,
        hints: [true, false, true],
    },
    Capability {
        name: "install",
        description: "Write and load the launchd agent that runs `board --personal autosync run` \
            every 300 seconds, or --interval, and at load. --dry-run prints its plist.",
        run: agent::install,
        input: input::<agent::InstallInput>,
        output: output::<agent::Output>,
        hints: [false, false, true],
    },
    Capability {
        name: "uninstall",
        description: "Unload the launchd agent and delete its plist.",
        run: agent::uninstall,
        input: input::<agent::UninstallInput>,
        output: output::<agent::Output>,
        hints: [false, true, true],
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
                    "openWorldHint": true,
                },
                "_meta": {},
            })
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Keep each board the owner listed in step with its remote on an interval, \
            pushing only commits a gitleaks scan passed. Use it to see why a board stopped \
            syncing, with `board --personal autosync status`. Only the owner adds a board, with \
            `board --personal autosync add`, which asks for Touch ID. SKILL.md and README.md in \
            the plugin's directory say more.",
        "metadata": {"area": "delivery"},
        "types": [BoardRecord::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;

//! The params plugin built into Board (plugins/params/README.md): calibrated
//! values, each with the heuristic that derives it and the evidence that set
//! it. `params.get` resolves a value, or the heuristic's default and why it
//! applies; `params.due` lists the values an agent should derive again.
//! Board stores the values and resolves them; an agent runs each method.
//! A heuristic the plugin ships applies until a record of its name overrides
//! it.

mod due;
mod get;
pub mod records;
mod stored;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

pub use get::{Got, Reason, Source, Warning, WarningCode, get};
use records::{HeuristicRecord, ValueRecord};

/// The heuristics the plugin ships, by name: the two overflow graces. A
/// record at `/params/heuristics/<name>` overrides the one of that name.
/// The files hold no `$type`, so `board check` in Board's own repository does
/// not read them as records of a type it has no schema for.
pub const SHIPPED: [(&str, &str); 2] = [
    (
        "overflow-agent-grace",
        include_str!("../heuristics/overflow-agent-grace.json"),
    ),
    (
        "overflow-owner-grace",
        include_str!("../heuristics/overflow-owner-grace.json"),
    ),
];

#[derive(Clone, Copy, Debug)]
pub struct Params;

impl Plugin for Params {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        let heuristic = (HeuristicRecord::TYPE.parse())
            .expect("HeuristicRecord's TYPE is a record type; a test parses it");
        let value = (ValueRecord::TYPE.parse())
            .expect("ValueRecord's TYPE is a record type; a test parses it");
        vec![
            (heuristic, schema_of::<HeuristicRecord>(settings())),
            (value, schema_of::<ValueRecord>(settings())),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "get" => get::call(host, &input),
            "due" => due::due(host, &input),
            other => Err(PluginError {
                code: "params::call::unknown_capability".to_owned(),
                message: format!("params offers no capability {other}; it offers get and due"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    let settings = SchemaSettings::draft2020_12;
    let read_only = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    });
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "params",
        "version": "0.1.0",
        "description": "Calibrated values, such as overflow's graces, each with the heuristic that \
            derives it and the run that set it. Use it to read a value a plugin uses, and when a \
            scheduled run asks you to recalibrate. Read one with `board call params.get --name \
            <name>`, which gives the heuristic's default and why when no value applies. `board call \
            params.due` lists the values to derive again. SKILL.md and README.md in the plugin's \
            directory say more.",
        "metadata": {"area": "governance"},
        "types": [HeuristicRecord::TYPE, ValueRecord::TYPE],
        "capabilities": [{
            "name": "get",
            "description": "A calibrated value for a scope, or its heuristic's default and why the \
                default applies.",
            "inputSchema": schema_of::<get::Input>(settings()),
            "outputSchema": schema_of::<Got>(settings().for_serialize()),
            "annotations": read_only,
            "_meta": {},
        }, {
            "name": "due",
            "description": "The values an agent should derive again, oldest first: those on the \
                default, those readers cannot use, those whose latest check failed, and those \
                older than their recheck_after in calendar days.",
            "inputSchema": schema_of::<due::Input>(settings()),
            "outputSchema": schema_of::<due::Output>(settings().for_serialize()),
            "annotations": read_only,
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;

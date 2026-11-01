//! The boundary plugin built into Board (plugins/boundary/README.md): it
//! checks content against the controls that boundaries in the personal
//! board name for where the content goes, before it leaves a checkout.

mod boundaries;
mod check;
mod content;
mod crossing;
pub mod records;

use board_model::{CapabilityName, RecordType, Refusal};
use board_ops::{Elsewhere, Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{BoundaryRecord, CrossingRecord};

/// How long `check` may take, in seconds: as long as `controls.evaluate`,
/// whose judged rules wait on a model.
const CHECK_TIMEOUT: u64 = 1800;
/// The variables `controls.evaluate` reads for a judged rule, passed through
/// to it.
const AGENT_VARIABLES: [&str; 3] = [
    "BOARD_CONTROLS_AGENT",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

#[derive(Clone, Copy, Debug)]
pub struct Boundary;

impl Plugin for Boundary {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        [
            (BoundaryRecord::TYPE, record_schema::<BoundaryRecord>()),
            (CrossingRecord::TYPE, record_schema::<CrossingRecord>()),
        ]
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
            "check" => check::check(host, &input),
            "verify" => crossing::verify(host, &input),
            "record" => crossing::record(host, &input),
            other => Err(PluginError {
                code: "boundary::call::unknown_capability".to_owned(),
                message: format!(
                    "boundary offers no capability {other}; it offers check, verify and record"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn record_schema<T: schemars::JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12())
}

/// The personal board, which holds the boundaries, their controls and the
/// crossings, whichever board called.
fn personal(host: &dyn Host) -> Result<Box<dyn Host + '_>, PluginError> {
    host.board(&Elsewhere::Personal)
        .map_err(|refusal| refused("--personal", &refusal))
}

/// `boundary::board::refused`, for Board's refusal of `board <command>`.
fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused("boundary", command, refusal)
}

fn manifest() -> Value {
    let settings = || SchemaSettings::draft2020_12().for_serialize();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "boundary",
        "version": "0.1.0",
        "description": "The boundary plugin checks content, such as a pull request body, before it \
            leaves a checkout, against the controls that boundaries in the personal board name for \
            its destination. Use it before you send anything a boundary may cover. Run `board \
            --personal call boundary.check` with the action, destination, checkout and parts on \
            stdin; on `boundary::check::failed`, change what it names and check again. SKILL.md and \
            README.md in the plugin's directory say more.",
        "metadata": {"area": "governance"},
        "types": [BoundaryRecord::TYPE, CrossingRecord::TYPE],
        "capabilities": [{
            "name": "check",
            "description": "Check content against every boundary that matches where it goes, and \
                record the crossing in the personal board. Fails naming each failing control, part \
                and line.",
            "inputSchema": schema_of::<check::Input>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<check::Output>(settings()),
            "annotations": {"readOnlyHint": false},
            "_meta": {},
            "timeout": CHECK_TIMEOUT,
            "env": AGENT_VARIABLES,
        }, {
            "name": "verify",
            "description": "Whether a passing crossing exists for a digest, or for given content.",
            "inputSchema": schema_of::<crossing::VerifyInput>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<crossing::VerifyOutput>(settings()),
            "annotations": {"readOnlyHint": true},
            "_meta": {},
        }, {
            "name": "record",
            "description": "Add what the destination returned, such as a URL, to a passing crossing.",
            "inputSchema": schema_of::<crossing::RecordInput>(SchemaSettings::draft2020_12()),
            "outputSchema": schema_of::<crossing::RecordOutput>(settings()),
            "annotations": {"readOnlyHint": false},
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;

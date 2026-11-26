//! The orders plugin built into Board (plugins/attention/README.md), named
//! attention before: orders the owner signs with Touch ID, which bind an
//! agent until they expire. `verify` and `pinned` only read; `sign` asks the owner through
//! `Host::sign_order` and writes the signature. The plugin never builds an
//! order's statement or runs the signer itself
//! (docs/architecture/writes/signing.md).

mod order;
mod pinned;
pub mod records;
mod sign;
mod signers;
mod text;
mod verify;

use board_model::{CapabilityName, ORDER_TYPE, RecordType, Refusal};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::schema_of;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug)]
pub struct Orders;

/// How long `sign` may take: the signer's 10 minutes and 10 seconds more
/// to read the order and write the signature.
const SIGN_TIMEOUT: u64 = 610;

impl Plugin for Orders {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (ORDER_TYPE.parse()).expect("ORDER_TYPE is a record type; a test parses it");
        vec![(name, records::record_schema())]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "verify" => verify::verify(host, &input),
            "pinned" => pinned::pinned(host, &input),
            "sign" => sign::sign(host, &input),
            "signers" => signers::signers(host, &input),
            other => Err(PluginError {
                code: "orders::call::unknown_capability".to_owned(),
                message: format!(
                    "orders offers no capability {other}; it offers verify, pinned, sign and signers"
                ),
                fields: Map::new(),
                hint: None,
            }),
        }
    }

    fn text(
        &self,
        capability: &CapabilityName,
        output: &Value,
        home: Option<&std::path::Path>,
    ) -> Option<String> {
        text::text(capability.as_str(), output, home)
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

/// `orders::board::refused`, for Board's refusal of `board <command>`.
pub(crate) fn refused(command: &str, refusal: &Refusal) -> PluginError {
    PluginError::board_refused("orders", command, refusal)
}

fn manifest() -> Value {
    let input = SchemaSettings::draft2020_12;
    let output = || SchemaSettings::draft2020_12().for_serialize();
    let reads = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    });
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "orders",
        "version": "0.3.0",
        // `attention.*` calls and `attention` in a board's plugin list reach
        // this plugin, with a warning naming `orders`, until Board drops the
        // name in its next release.
        "former_names": ["attention"],
        // SKILL.md's description, the callout an agent sees first.
        "description": "Orders the owner signed with Touch ID, which bind an agent until they \
            expire. Pinned orders normally arrive in your context when a session starts. Only \
            when that section is missing, read them with `board --personal \
            orders pinned`. Use when you need to check one: `board --personal \
            orders verify --path /orders/<name>`. Use also when you have drafted an order for \
            the owner to sign. SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "authority"},
        "types": [ORDER_TYPE],
        "capabilities": [{
            "name": "verify",
            "description": "An order and its context, when the owner's key signed it and it has \
                not expired; otherwise the reason it cannot be trusted.",
            "inputSchema": schema_of::<verify::Input>(input()),
            "outputSchema": schema_of::<verify::Output>(output()),
            "annotations": reads,
            "_meta": {},
        }, {
            "name": "pinned",
            "description": "Every pinned order in the board's own tree that verifies, for an \
                agent session to start with, and the code of each one left out.",
            "inputSchema": schema_of::<pinned::Input>(input()),
            "outputSchema": schema_of::<pinned::Output>(output()),
            "annotations": reads,
            "_meta": {},
        }, {
            "name": "sign",
            "description": "Show the owner an order in the signing window and, once they sign \
                it with Touch ID, write their signature into it. Refuses with the owner's \
                requested changes when they ask for some.",
            "inputSchema": schema_of::<sign::Input>(input()),
            "outputSchema": schema_of::<sign::Output>(output()),
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false,
            },
            "timeout": SIGN_TIMEOUT,
            "_meta": {},
        }, {
            "name": "signers",
            "description": "Board's allowed signers file with each key of the attention allowed \
                signers file merged in for orders, which the owner reads and writes to Board's \
                file themselves. It writes neither file.",
            "inputSchema": schema_of::<signers::Input>(input()),
            "outputSchema": schema_of::<signers::Output>(output()),
            "annotations": reads,
            "_meta": {},
        }],
    })
}

#[cfg(test)]
mod tests;

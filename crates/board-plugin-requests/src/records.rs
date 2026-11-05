//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Digest, Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

/// `requests/reply/v1`: the owner's reply to a pending effect. It approves
/// nothing.
///
/// It lives at `/requests/replies/<effect id>/<time>` in the personal board,
/// where `<time>` is a sortable UTC time such as `20261030T224454Z`, so the
/// file name orders the replies.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The id of the effect the reply answers, as a digest: `sha256:` and 64
    /// lowercase hex digits. The 64 digits alone, which Board.app's relay
    /// writes, are admitted too until it writes the digest.
    #[schemars(schema_with = "effect_id")]
    pub effect: String,
    /// What the owner says.
    #[schemars(length(min = 1))]
    pub text: String,
}

/// A [`Digest`], or the bare 64 hex digits Board.app's relay writes.
fn effect_id(generator: &mut SchemaGenerator) -> Schema {
    let digest = generator.subschema_for::<Digest>();
    json_schema!({"anyOf": [digest, {"type": "string", "pattern": "^[0-9a-f]{64}$"}]})
}

impl Record for ReplyRecord {
    const TYPE: &'static str = "requests/reply/v1";
}

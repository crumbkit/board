//! The order record and the schema the plugin ships for it.

use board_model::ORDER_TYPE;
use serde::Deserialize;
use serde_json::{Value, json};

/// An order as the plugin reads it: the fields of an order record other
/// than `$type`, which the reader checks first. A field the type does not
/// name is ignored here, as the Python plugin ignored it; Board's schema
/// check refuses it on put.
#[derive(Debug, Deserialize)]
pub struct OrderRecord {
    /// The binding instruction.
    pub order: String,
    /// The path of the Markdown file that holds the non-binding context.
    pub context: String,
    /// The last day the order binds, as `YYYY-MM-DD`.
    pub expires: String,
    /// Whether every agent session starts with the order.
    #[serde(default)]
    pub pinned: bool,
    /// The owner's signature of the order's statement; none on a draft.
    pub signature: Option<String>,
}

/// The schema of the order record, as the Python plugin generated it with
/// pydantic: every property titled, the field meanings in the type's
/// description, and no `$schema`. `schema_of` cannot write those titles,
/// so the schema is written out here, and a test compares it with the
/// committed file.
pub(crate) fn record_schema() -> Value {
    let description = format!(
        "`{ORDER_TYPE}`: an instruction the owner signed, which agents may act on until it \
         expires.\n\n`order` is the binding instruction. A `pinned` order is shown to every\n\
         agent session as it starts. `context` references a Markdown file\nholding the \
         non-binding context from the conversation that produced it.\n`signature` is the \
         owner's SSH signature, in the `{ORDER_TYPE}`\nnamespace, of the order's statement, \
         which holds the order, its expiry,\nwhether it is pinned, and the context's text. An \
         order without one is a draft."
    );
    json!({
        "additionalProperties": false,
        "description": description,
        "properties": {
            "$type": {"const": ORDER_TYPE, "title": "$Type", "type": "string"},
            "context": {"format": "board-ref", "title": "Context", "type": "string"},
            "expires": {"format": "date", "title": "Expires", "type": "string"},
            "order": {"minLength": 1, "title": "Order", "type": "string"},
            "pinned": {"title": "Pinned", "type": "boolean"},
            "signature": {"title": "Signature", "type": "string"},
        },
        "required": ["$type", "order", "context", "expires"],
        "title": "OrderRecord",
        "type": "object",
    })
}

//! The record type the plugin ships, and the budget its outputs repeat. The
//! schema is generated from these structs; the file under `types/` is that
//! schema, committed. It keeps the bytes the Python plugin's pydantic shapes
//! generated, so a board that enabled controls before the port needs no
//! `plugin enable`: every property has a title, and `rule` admits null.

use board_plugin::shape::{Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `controls/control/v1`: a requirement on files, why it exists, and
/// optionally a budget a program checks.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "ControlRecord",
    description = "`controls/control/v1`: a requirement on files, why it exists, and optionally a budget a program checks.\n\nIt applies to the entries `applies_to` references and to every entry of\nthe board whose path matches one of `paths`, SQLite GLOB patterns such as\n`docs/*.md`."
)]
pub struct ControlRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Requirement")]
    pub requirement: String,
    #[schemars(title = "Purpose")]
    pub purpose: String,
    // A field left out admits no null: `type` replaces Option's `["array", "null"]`.
    #[schemars(title = "Applies To", extend("type" = "array"))]
    pub applies_to: Option<Vec<Ref>>,
    #[schemars(title = "Paths", extend("type" = "array"))]
    pub paths: Option<Vec<String>>,
    #[serde(default)]
    pub rule: Option<Budget>,
}

impl Record for ControlRecord {
    const TYPE: &'static str = "controls/control/v1";
}

/// A limit on the words or bytes of the files the control applies to.
///
/// With `per` `total`, the default, the files' usage is summed; with `file`,
/// each file on its own must be within `max`. Words are separated by any
/// whitespace.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Budget")]
pub struct Budget {
    #[schemars(title = "Budget")]
    pub budget: Measure,
    #[schemars(title = "Max")]
    pub max: u64,
    // Left out when the record leaves it out, so an output repeats the rule
    // as the record holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Per", with = "Per", extend("default" = "total"))]
    pub per: Option<Per>,
}

// What a budget counts. No doc comment, which would describe the inlined
// enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Measure {
    Words,
    Bytes,
}

// Whether a budget sums its files or limits each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Per {
    Total,
    File,
}

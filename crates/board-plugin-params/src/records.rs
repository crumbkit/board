//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.
//! The plugin reads both through `serde_json::Value`, so these structs only
//! describe.

use std::collections::BTreeMap;

use board_plugin::shape::{Actor, Prose, Record, Ref, Slug, Summary, Tag, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `params/heuristic/v1`: how an agent derives one calibrated value, at
/// `/params/heuristics/<name>`. It names its inputs and its method, the
/// estimate the method starts from, the lenient default used without enough
/// data, the bounds a value must keep, and the check that says when the
/// value is wrong.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeuristicRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What the value is for.
    #[schemars(with = "Summary")]
    pub summary: String,
    /// The value's unit, such as `active-days` or `changed-lines`.
    #[schemars(with = "Slug")]
    pub unit: String,
    /// Each input the method reads.
    #[schemars(length(min = 1))]
    pub inputs: Vec<Input>,
    /// How to derive the value from the inputs.
    #[schemars(with = "Prose")]
    pub method: String,
    /// The estimate the method starts from.
    pub start: f64,
    /// The lenient value readers use without enough data.
    pub default: f64,
    /// The least and the greatest value admitted.
    pub bounds: Bounds,
    /// What counts as enough data to derive a value.
    #[schemars(with = "Prose")]
    pub enough: String,
    /// The measure that says when the value is wrong.
    pub check: Check,
    /// The active days after which the value is due again.
    #[schemars(range(min = 1))]
    pub recheck_after: u64,
    /// The plugin that reads the value, such as `controls`.
    #[schemars(with = "Slug")]
    pub consumer: String,
}

impl Record for HeuristicRecord {
    const TYPE: &'static str = "params/heuristic/v1";
}

/// One input of a heuristic.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// The input's name, such as `authored-lines-per-day`.
    #[schemars(with = "Slug")]
    pub name: String,
    /// What the input measures.
    #[schemars(with = "Summary")]
    pub meaning: String,
    /// The command that measures it, and how.
    #[schemars(with = "Prose")]
    pub source: String,
}

/// The least and the greatest value a heuristic admits.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    /// The least value admitted.
    pub min: f64,
    /// The greatest value admitted.
    pub max: f64,
}

/// The check on a value: when `measure` passes `fails_above`, the value is
/// wrong.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// What the check measures, with the command that computes it.
    #[schemars(with = "Prose")]
    pub measure: String,
    /// The measure above which the value is wrong.
    pub fails_above: f64,
}

/// `params/value/v1`: the value an agent derived by running a heuristic, at
/// `/params/values/<name>`, the same `<name>` as its heuristic. With
/// `data` `too-little`, readers use the heuristic's default.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValueRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The heuristic run, `/params/heuristics/<name>`.
    pub heuristic: Ref<HeuristicRecord>,
    /// The value for any scope `scopes` does not list.
    pub value: f64,
    /// The value for each repository or project, by its path, such as
    /// `/repos/board`.
    #[schemars(extend("propertyNames" = {"pattern": "^/\\S*$"}))]
    pub scopes: Option<BTreeMap<String, f64>>,
    /// Whether the run found enough data.
    pub data: Data,
    /// The run of `/suites/params-<name>` that holds the inputs and the
    /// check the agent measured.
    pub evidence: Ref<Run>,
    /// When the agent ran the heuristic.
    #[schemars(with = "Timestamp")]
    pub derived: String,
    /// Who ran it.
    pub by: Actor,
}

impl Record for ValueRecord {
    const TYPE: &'static str = "params/value/v1";
}

/// A `runs/run/v1` record, which the runs plugin ships.
#[derive(Debug)]
pub enum Run {}

impl Record for Run {
    const TYPE: &'static str = "runs/run/v1";
}

/// Whether a heuristic's run found enough data. With `too-little`, readers
/// use the heuristic's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Data {
    Enough,
    TooLittle,
}

//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Record, Ref, Tag, Url};
use schemars::JsonSchema;

/// `decisions/decision/v1`: a choice and why it was made, written once.
///
/// Nothing on it changes after it is written. Whether it awaits review, was
/// reviewed or is superseded is derived from the questions whose `for`
/// names it and the decisions whose `supersedes` names it.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/projects/{project}/decisions/"))]
pub struct DecisionRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The choice, in one sentence.
    #[schemars(length(min = 1))]
    pub decision: String,
    /// Why it was made.
    #[schemars(length(min = 1))]
    pub why: String,
    /// What was not chosen.
    #[serde(default)]
    pub alternatives: Vec<String>,
    /// When it was decided.
    #[schemars(extend("format" = "date-time"))]
    pub decided: String,
    /// The session that decided.
    #[schemars(length(min = 1))]
    pub session: String,
    /// The records the decision rests on, such as an eval, an owner's
    /// answer, a review report, an earlier decision or a file in a mounted
    /// repository.
    #[serde(default)]
    pub basis: Vec<Ref>,
    /// Web pages the decision concerns, such as a pull request.
    #[serde(default)]
    #[schemars(with = "Vec<Url>")]
    pub urls: Vec<String>,
    /// The decisions this one replaces.
    #[serde(default)]
    pub supersedes: Vec<Ref<Self>>,
}

impl Record for DecisionRecord {
    const TYPE: &'static str = "decisions/decision/v1";
}

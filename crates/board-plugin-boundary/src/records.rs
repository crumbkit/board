//! The record types the plugin ships, and the verdicts a crossing repeats.
//! Their schemas are generated from these structs; the files under `types/`
//! are those schemas, committed.

use board_plugin::shape::{Glob, Prose, Record, Ref, Sha256, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `boundary/boundary/v1`: the controls content must pass to leave a
/// checkout for a destination by an action.
///
/// Boundaries belong under `/boundaries/` in the personal board. Globs are
/// `SQLite` GLOB patterns: `*` matches any characters, `/` and `.` included.
/// Destinations match without case; action and part names match with case.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The checkouts it applies to, by their paths, usually mounts; left out
    /// for any checkout.
    #[schemars(with = "Option<Vec<Ref>>")]
    pub from: Option<Vec<String>>,
    /// A glob of the destinations it applies to, such as github:octo/*.
    #[schemars(with = "Glob")]
    pub to: String,
    /// Globs of the actions it applies to, such as github.*.
    #[schemars(with = "Vec<Glob>", length(min = 1))]
    pub actions: Vec<String>,
    /// The controls to check and the parts each applies to.
    #[schemars(length(min = 1))]
    pub checks: Vec<Check>,
}

impl Record for BoundaryRecord {
    const TYPE: &'static str = "boundary/boundary/v1";
}

/// A control, and the names of the parts it applies to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// The control to check the parts against, in the personal board's own
    /// tree.
    #[schemars(with = "Ref<Control>")]
    pub control: String,
    /// Globs of the part names it applies to, such as pr.* or *.
    #[schemars(with = "Vec<Glob>", length(min = 1))]
    pub parts: Vec<String>,
}

/// The controls plugin's record, which a check and a verdict name.
#[derive(Debug)]
pub enum Control {}

impl Record for Control {
    const TYPE: &'static str = "controls/control/v1";
}

/// `boundary/crossing/v1`: the verdicts on one content, at
/// `/crossings/<digest>` in the personal board.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrossingRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The SHA-256 of the content's action, destination and parts.
    #[schemars(with = "Sha256")]
    pub digest: String,
    /// The capability that sends the parts, such as github.pr-create.
    pub action: String,
    /// Where the parts go, such as github:octo/board.
    pub to: String,
    /// The checkout the parts leave, by its path in the personal board; left
    /// out when the check named none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref>")]
    pub from: Option<String>,
    /// Whether every check passed, or the owner approved an override.
    pub passed: bool,
    /// The boundaries that matched, by their paths.
    #[schemars(with = "Vec<Ref<BoundaryRecord>>")]
    pub boundaries: Vec<String>,
    /// Each check's verdict on each part it applies to.
    pub verdicts: Vec<Verdict>,
    /// What the destination returned, such as a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub result: Option<String>,
    /// The id of the owner's approved effect that let this content through
    /// its failed checks.
    #[serde(rename = "override", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Sha256>")]
    pub overridden: Option<String>,
}

impl Record for CrossingRecord {
    const TYPE: &'static str = "boundary/crossing/v1";
}

/// One control's verdict on one part, as `controls.evaluate` gave it, with
/// each null left out.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    /// The control's path.
    #[schemars(with = "Ref<Control>")]
    pub control: String,
    /// The part's name.
    pub part: String,
    /// The kind of the control's rule; left out for a written requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<Kind>,
    /// passed, failed, skipped after a free rule failed, or stated.
    pub status: Status,
    /// What the control found wrong, at most 20.
    pub findings: Vec<Finding>,
    /// What a budget counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<u64>,
    /// A budget's limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
    /// How many findings were left out past the first 20.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<u64>,
}

/// The kind of rule a verdict comes from.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Kind {
    Budget,
    Pattern,
    Judged,
}

/// How a part fared against one control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Status {
    Passed,
    Failed,
    Skipped,
    Stated,
}

/// What a control found wrong in a part: the text at fault, and its line
/// when it has one.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// The text at fault: a match, a budget's count, or a quoted sentence.
    #[schemars(with = "Prose")]
    pub text: String,
    /// The line of the part the text starts on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// The expression that matched, when its control shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub pattern: Option<String>,
    /// How to rewrite the text, from a judged rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub rewrite: Option<String>,
}

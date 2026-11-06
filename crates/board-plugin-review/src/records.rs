//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.
//! They keep the bytes the Python plugin shipped, with a title on every
//! property as pydantic wrote it, so a board that enabled review before the
//! port needs no `plugin enable`.

use board_plugin::shape::{Actor, Open, Pinned, Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "BriefRecord",
    description = "`review/brief/v1`, at /reviews/<id>/brief: what a change's author asks a review to look at.\n\n`range` is the commits under review as `base..head`, both full commit ids,\nand `subject` the record under review at its blob; a brief has one of them.\n`focus` says what matters and what to check first, `questions` what the\nauthor wants answered, and `follows` the report this brief continues from.",
    // A brief names a range of commits or a subject, never both.
    extend("oneOf" = [{"required": ["range"]}, {"required": ["subject"]}])
)]
pub struct BriefRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "commits")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Pinned>,
    #[schemars(schema_with = "focus")]
    pub focus: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "questions")]
    pub questions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "follows")]
    pub follows: Option<String>,
}

impl Record for BriefRecord {
    const TYPE: &'static str = "review/brief/v1";
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "ReportRecord",
    description = "`review/report/v1`, at /reviews/<id>/report: what one review of a range or a record found.\n\n`range` holds the full commit ids reviewed, and `patch_ids` the\n`git patch-id --verbatim` of each commit in it that has a diff, oldest first,\nso the review still covers the commits after a rebase. A review of a record\nholds its `subject` instead, and covers no commit. `brief` is absent for\na review a person wrote. `by` names who reviewed: the program the agent\ncommand starts, its session, or the person. `source` is a URL, such as\na GitHub review. `follows` is the previous report.",
    extend(
        "oneOf" = [{"required": ["range"]}, {"required": ["subject"]}],
        "dependentRequired" = {"range": ["patch_ids"], "patch_ids": ["range"]}
    )
)]
pub struct ReportRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "brief")]
    pub brief: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "commits")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "patch_ids")]
    pub patch_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Pinned>,
    #[schemars(title = "Findings")]
    pub findings: Vec<Finding>,
    #[schemars(schema_with = "verdict")]
    pub verdict: Open<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Actor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "source")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "follows")]
    pub follows: Option<String>,
}

impl Record for ReportRecord {
    const TYPE: &'static str = "review/report/v1";
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Finding",
    description = "One thing a review found, at a file and line when it has one."
)]
pub struct Finding {
    #[schemars(schema_with = "severity")]
    pub severity: Open<Severity>,
    #[schemars(schema_with = "text")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "file")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "line")]
    pub line: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Cost",
    description = "What a review took: its seconds, and its tokens when a model ran and said."
)]
pub struct Cost {
    #[schemars(schema_with = "seconds")]
    pub seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "input_tokens")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "output_tokens")]
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Approve,
    Changes,
    Comment,
}

impl Verdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Changes => "changes",
            Self::Comment => "comment",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// A range of full commit ids, so a record names exactly which commits it
/// means and cannot pass git an option.
pub(crate) const COMMITS: &str = r"^[0-9a-f]{40,64}\.\.[0-9a-f]{40,64}$";

fn commits(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"pattern": COMMITS, "title": "Range", "type": "string"})
}

fn patch_ids(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "items": {"pattern": "^[0-9a-f]{40,64}$", "type": "string"},
        "title": "Patch Ids",
        "type": "array",
    })
}

fn focus(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minLength": 1, "title": "Focus", "type": "string"})
}

fn questions(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"items": {"type": "string"}, "title": "Questions", "type": "array"})
}

fn reference(title: &str, record_type: &str) -> Schema {
    json_schema!({"format": "board-ref", "title": title, "type": "string", "x-board-type": record_type})
}

fn follows(_: &mut SchemaGenerator) -> Schema {
    reference("Follows", ReportRecord::TYPE)
}

fn brief(_: &mut SchemaGenerator) -> Schema {
    reference("Brief", BriefRecord::TYPE)
}

pub(crate) fn verdict(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["approve", "changes", "comment"], "title": "Verdict", "type": "string"})
}

fn severity(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["high", "medium", "low"], "title": "Severity", "type": "string"})
}

fn text(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minLength": 1, "title": "Text", "type": "string"})
}

fn file(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"title": "File", "type": "string"})
}

fn line(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minimum": 1, "title": "Line", "type": "integer"})
}

fn source(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"pattern": "^https?://", "title": "Source", "type": "string"})
}

fn seconds(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minimum": 0, "title": "Seconds", "type": "number"})
}

fn input_tokens(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minimum": 0, "title": "Input Tokens", "type": "integer"})
}

fn output_tokens(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"minimum": 0, "title": "Output Tokens", "type": "integer"})
}

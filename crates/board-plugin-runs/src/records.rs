//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.
//! They keep the bytes the Python plugin's pydantic models generated, so a
//! board that enabled runs before the port needs no `plugin enable`: every
//! property has a title, and field meanings live in each type's description.
//! The plugin reads a run through `qualifying::StoredRun`, which ignores the
//! fields a board's looser schema may add, so these structs only describe.

use std::collections::BTreeMap;

use board_plugin::shape::{Record, Ref, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;
use serde_json::{Map, Value};

/// `runs/suite/v1`: what gets run, how, and what a run of it measures.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "SuiteRecord",
    description = "`runs/suite/v1`: what gets run, how, and what a run of it measures.\n\nA suite's name is its path: the suite at `/suites/<name>.json` is\n`<name>`, and its runs sit in `/suites/<name>/runs/`. `subjects` are the\nnames of what is tested. `cases` lists the cases inline, or `cases_file`\nreferences a file that does; a suite whose cases are found when it runs,\nsuch as a test suite, has neither.",
    extend("not" = {"required": ["cases", "cases_file"]})
)]
pub struct SuiteRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Subjects", length(min = 1))]
    pub subjects: Vec<String>,
    #[schemars(title = "Method")]
    pub method: String,
    // A field left out admits no null: `type` replaces Option's `["string", "null"]`.
    #[schemars(title = "Command", extend("type" = "string"))]
    pub command: Option<String>,
    #[schemars(title = "Cases", extend("type" = "array"))]
    pub cases: Option<Vec<Case>>,
    #[schemars(title = "Cases File", extend("type" = "string"))]
    pub cases_file: Option<Ref>,
    #[schemars(title = "Measures", extend("type" = "array"))]
    pub measures: Option<Vec<Measure>>,
}

impl Record for SuiteRecord {
    const TYPE: &'static str = "runs/suite/v1";
}

/// One case of a suite. A run's result names it by `id`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Case")]
pub struct Case {
    #[schemars(title = "Id")]
    pub id: String,
    #[schemars(title = "Input")]
    pub input: Option<Value>,
    #[schemars(title = "Expected")]
    pub expected: Option<Value>,
}

/// A quantity a run takes, in `unit`. A result or a run gives its value under `name`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Measure")]
pub struct Measure {
    #[schemars(title = "Name")]
    pub name: String,
    #[schemars(title = "Unit")]
    pub unit: String,
    #[schemars(title = "Description", extend("type" = "string"))]
    pub description: Option<String>,
}

/// `runs/run/v1`: one run of a suite, by whom, when, on what, and what it gave.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "RunRecord",
    description = "`runs/run/v1`: one run of a suite, by whom, when, on what, and what it gave.\n\n`scope` is `full` when the run ran every case and `targeted` when it ran\nsome. `dirty` says whether the working tree under test had uncommitted\nchanges; a run that does not say is never counted as clean. `versions`\nmaps what was used, such as a commit, a model or a library, to its\nversion. `setup` says how this run was prepared, and `summary` what it\nshowed. `measures` holds the run's own measures, such as `tests` and\n`failures`. A test suite's run lists only the cases that did not pass and\nthe cases that measured something; an eval's run may list every case.\n`written_by` names the plugin and hook that wrote a run without an agent.",
    extend("x-board-folder" = "/suites/{suite}/runs/")
)]
pub struct RunRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Suite")]
    pub suite: Ref<SuiteRecord>,
    #[schemars(title = "By")]
    pub by: String,
    #[schemars(title = "Started", extend("format" = "date-time"))]
    pub started: String,
    #[schemars(title = "Scope")]
    pub scope: Scope,
    #[schemars(title = "Results")]
    pub results: Vec<CaseResult>,
    #[schemars(title = "Dirty", extend("type" = "boolean"))]
    pub dirty: Option<bool>,
    #[schemars(title = "Finished", extend("type" = "string", "format" = "date-time"))]
    pub finished: Option<String>,
    #[schemars(title = "Versions", extend("type" = "object"))]
    pub versions: Option<BTreeMap<String, String>>,
    #[serde(default)]
    #[schemars(with = "Host")]
    pub host: Option<Host>,
    #[schemars(title = "Measures", with = "Option<BTreeMap<String, MeasureValue>>", extend("type" = "object"))]
    pub measures: Option<Map<String, Value>>,
    #[schemars(title = "Setup", extend("type" = "string"))]
    pub setup: Option<String>,
    #[schemars(title = "Summary", extend("type" = "string"))]
    pub summary: Option<String>,
    #[schemars(
        title = "Written By",
        description = "The plugin and hook that wrote this record, such as runs/subagent-stop; absent when an agent or a person wrote it.",
        regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"),
        extend("type" = "string")
    )]
    pub written_by: Option<String>,
}

impl Record for RunRecord {
    const TYPE: &'static str = "runs/run/v1";
}

// Whether a run ran every case. No doc comment, which would describe the
// inlined enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Scope {
    Full,
    Targeted,
}

/// The machine a run ran on.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Host")]
pub struct Host {
    #[schemars(title = "Os")]
    pub os: String,
    #[schemars(title = "Arch")]
    pub arch: String,
    #[schemars(title = "Logical Cpus", range(min = 1))]
    pub logical_cpus: u32,
}

/// What one case gave in a run. A case that did not pass has a `note`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    rename = "Result",
    title = "Result",
    description = "What one case gave in a run. A case that did not pass has a `note`.\n\n`trial` numbers repeated trials of one case, from 1. `evidence` references files in the board that back the\nresult, such as a log an eval kept.",
    extend(
        "anyOf" = [{"required": ["outcome"]}, {"required": ["measures"]}],
        "if" = {"properties": {"outcome": {"enum": ["fail", "error", "skipped"]}}, "required": ["outcome"]},
        "then" = {"required": ["note"]},
    )
)]
pub struct CaseResult {
    #[schemars(title = "Case")]
    pub case: String,
    #[serde(default)]
    #[schemars(title = "Outcome", with = "Outcome")]
    pub outcome: Option<Outcome>,
    #[schemars(title = "Measures", with = "Option<BTreeMap<String, MeasureValue>>", extend("type" = "object"))]
    pub measures: Option<Map<String, Value>>,
    #[schemars(title = "Note", extend("type" = "string"))]
    pub note: Option<String>,
    #[schemars(title = "Trial", range(min = 1), extend("type" = "integer"))]
    pub trial: Option<u32>,
    #[schemars(title = "Evidence", extend("type" = "array"))]
    pub evidence: Option<Vec<Ref>>,
}

// What a case gave. No doc comment, which would describe the inlined enum
// in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Outcome {
    Pass,
    Fail,
    Error,
    Skipped,
}

impl Outcome {
    /// Whether the outcome makes a run fail.
    pub(crate) fn failed(self) -> bool {
        matches!(self, Self::Fail | Self::Error)
    }
}

/// What a case or a run measures: a number, a label or a flag.
#[derive(Debug)]
pub enum MeasureValue {}

impl JsonSchema for MeasureValue {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MeasureValue".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"anyOf": [
            {"type": "integer"},
            {"type": "number"},
            {"type": "string"},
            {"type": "boolean"},
        ]})
    }
}

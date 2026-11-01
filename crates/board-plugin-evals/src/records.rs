//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `evals/eval/v1`: a question about some subjects, how it is tested, what
/// it rests on, and what was concluded.
///
/// Its status is derived, never stored: `abandoned` once `abandoned` is
/// set, else `concluded` once it has a `verdict`, else `running` once it
/// lists `runs`, else `planned`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvalRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The question the eval answers.
    pub question: String,
    /// The names of what is tested, such as `crumbkit` or `clap`, without
    /// versions, which go in a run's `versions`. Name a repository by its
    /// repo record, in `repos`.
    #[schemars(length(min = 1))]
    pub subjects: Vec<String>,
    /// The repo record of each repository the eval concerns, such as
    /// `/repos/crumbkit`.
    pub repos: Option<Vec<Ref>>,
    /// The suites that say how the eval is tested.
    pub suites: Option<Vec<Ref<Suite>>>,
    /// The runs its verdict rests on.
    pub runs: Option<Vec<Ref<Run>>>,
    /// The question of the decision this eval informs, written before any
    /// decision exists.
    pub informs: Option<String>,
    /// What the runs show.
    pub verdict: Option<Verdict>,
    /// What was concluded, in words.
    pub conclusion: Option<String>,
    /// Why the eval was stopped before it concluded.
    #[schemars(length(min = 1))]
    pub abandoned: Option<String>,
    /// The plugin and hook that wrote this record, such as runs/test-run;
    /// absent when an agent or a person wrote it.
    #[schemars(regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"))]
    pub written_by: Option<String>,
}

impl Record for EvalRecord {
    const TYPE: &'static str = "evals/eval/v1";
}

/// A `runs/suite/v1` record, which the runs plugin ships.
#[derive(Debug)]
pub enum Suite {}

impl Record for Suite {
    const TYPE: &'static str = "runs/suite/v1";
}

/// A `runs/run/v1` record, which the runs plugin ships.
#[derive(Debug)]
pub enum Run {}

impl Record for Run {
    const TYPE: &'static str = "runs/run/v1";
}

/// What a concluded eval's runs show.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Verdict {
    Supported,
    Refuted,
    Inconclusive,
}

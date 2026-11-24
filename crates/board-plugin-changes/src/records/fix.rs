//! `changes/fix/v1`, the fix round after a review: which commits answer
//! which of the review's findings.

use std::num::NonZeroU32;

use board_plugin::shape::{CommitName, Prose, Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The record type of a review report; the review plugin owns it.
pub(crate) const REPORT_TYPE: &str = "review/report/v1";

/// A review report of the review plugin.
#[derive(Debug)]
pub(crate) enum Report {}

impl board_plugin::shape::Accepts for Report {
    const TYPES: &'static [&'static str] = &[REPORT_TYPE];
}

/// `changes/fix/v1`: the commits a builder made after a review to answer
/// its findings, each with the findings it answers.
///
/// It lives at `/changes/<name>/fixes/<n>`, attached to the change. A
/// review check whose `where` names `unchanged_by` accepts each commit it
/// lists after the report it names, when that report has no high finding,
/// every finding number it gives is one of the report's, and it records no
/// `deviation`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/fixes/"))]
pub(crate) struct FixRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The review report whose findings the commits answer.
    #[schemars(with = "Ref<Report>")]
    report: String,
    /// Each fix commit, with the findings it answers.
    #[schemars(extend("minItems" = 1))]
    commits: Vec<FixCommit>,
    /// How the fix departs from the plan, when it does. A fix that departs
    /// from the plan needs a new review of the head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", extend("minLength" = 1))]
    deviation: Option<String>,
}

impl Record for FixRecord {
    const TYPE: &'static str = "changes/fix/v1";
}

/// One fix commit and the findings it answers.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "FixCommit")]
struct FixCommit {
    /// The commit, whole or abbreviated.
    #[schemars(with = "CommitName")]
    commit: String,
    /// The numbers of the report's findings the commit answers, counting
    /// its first finding as 1.
    #[schemars(extend("minItems" = 1))]
    findings: Vec<NonZeroU32>,
}

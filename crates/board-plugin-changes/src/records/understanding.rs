//! `changes/understanding/v1`, the record of an understanding check.

use board_plugin::shape::{Prose, Record, Ref, Slug, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AnyChange;

/// `changes/understanding/v1`: the owner's answers to a check that he
/// understands a change's data or UX.
///
/// It lives at `/changes/<name>/understanding/<subject>`. The owner's
/// sign-off decision references it in its `refs`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/understanding/"))]
pub(crate) struct UnderstandingRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The change whose design was checked.
    #[schemars(with = "Ref<AnyChange>")]
    change: String,
    /// What was checked: the design's data or its UX.
    subject: Subject,
    /// Each question asked, in order.
    #[schemars(length(min = 1))]
    questions: Vec<Asked>,
    /// Predict or edge questions asked again, worded differently, before
    /// review. The build gate reads the latest recheck of each point and
    /// wants the flow's number of points, each matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rechecks: Option<Vec<Recheck>>,
}

impl Record for UnderstandingRecord {
    const TYPE: &'static str = "changes/understanding/v1";
}

/// What an understanding check covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Subject {
    Data,
    Ux,
}

/// One question of an understanding check, the owner's answer and the
/// design's.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Asked")]
struct Asked {
    /// The question, as asked.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    question: String,
    /// The owner's answer, in his own words.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    answer: String,
    /// What the design says the answer is.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    design: String,
    /// The kind of question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<Question>,
    /// Whether the owner's answer matches the design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matches: Option<bool>,
}

/// One question asked again before review: the owner's answer and the
/// design's, and the point of the design it checks.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Recheck")]
struct Recheck {
    /// The question, as asked.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    question: String,
    /// The owner's answer, in his own words.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    answer: String,
    /// What the design says the answer is.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    design: String,
    /// The kind of question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<Question>,
    /// Whether the owner's answer matches the design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matches: Option<bool>,
    /// A short name for the part of the design the recheck checks, such as
    /// `flow-version`. Rechecks of one point carry the same value, and the
    /// build gate reads only the latest of each point.
    #[schemars(with = "Slug")]
    point: String,
}

/// The kinds of question an understanding check asks.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Question {
    Predict,
    Choose,
    Explain,
    Edge,
}

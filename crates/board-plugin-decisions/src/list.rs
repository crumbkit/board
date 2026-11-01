//! `open` and `about`: decisions with their derived state, by the questions
//! that review them and the decisions that supersede them.

use std::cmp::Reverse;
use std::collections::HashMap;

use board_ops::{Host, PluginError};
use board_plugin::shape::whole;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::standing::{Decision, decisions, rows};

/// Every question that names what it belongs to.
pub(crate) const QUESTIONS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'plans/question/v1' AND logical IS NOT NULL \
     AND json_extract(body, '$.for') IS NOT NULL";

/// The decisions under `path` that await review, oldest first.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "OpenInput")]
pub(crate) struct OpenInput {
    /// Only decisions below this logical path, such as /projects/board;
    /// every decision when left out.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The most decisions to return, 1 to 10,000; 100 when left out.
    #[serde(deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10_000))]
    limit: u16,
}

impl Default for OpenInput {
    fn default() -> Self {
        Self {
            path: "/".to_owned(),
            limit: most(),
        }
    }
}

/// The decisions not superseded whose `basis` names a path or an entry
/// under it, newest first.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AboutInput")]
pub(crate) struct AboutInput {
    /// The path a decision's basis names, such as /evals/gix-refs, or a
    /// directory such as /repos/board/docs.
    #[serde(rename = "ref")]
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    target: String,
    /// The most decisions to return, 1 to 10,000; 100 when left out.
    #[serde(default = "most", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10_000))]
    limit: u16,
}

/// The default `limit`.
fn most() -> u16 {
    100
}

/// Decisions with their derived state. `total` counts every match, and
/// `truncated` says whether the limit left some out. `unreadable` counts the
/// decisions, and the questions with a `for`, that were left out because
/// Board marks them degraded or this plugin cannot read them, in this board
/// and its mounted boards.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListOutput")]
pub(crate) struct Output {
    decisions: Vec<Listed>,
    total: usize,
    truncated: bool,
    unreadable: usize,
}

/// A decision by path, with its derived state. `questions` are the open
/// questions whose `for` names it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Listed {
    path: String,
    decision: String,
    why: String,
    #[schemars(extend("format" = "date-time"))]
    decided: String,
    basis: Vec<String>,
    urls: Vec<String>,
    review: Review,
    questions: Vec<String>,
    superseded: bool,
}

/// Where a decision's review stands: `awaiting` while a question whose
/// `for` names it is open, else `reviewed` once one is answered, else
/// `unreviewed`. A withdrawn question counts for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Review {
    Unreviewed,
    Awaiting,
    Reviewed,
}

/// A question's status, as far as review reads it.
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Open,
    Answered,
    Withdrawn,
}

#[derive(Deserialize)]
struct Asked {
    status: Status,
    #[serde(rename = "for")]
    belongs_to: Vec<String>,
}

/// What the questions say about each decision they name: the open
/// questions, and whether one was answered.
#[derive(Default)]
struct Reviews {
    open: Vec<String>,
    answered: bool,
}

pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let OpenInput { path, limit } = PluginError::read_input("decisions", "open", input)?;
    let below = format!("{}/", path.trim_end_matches('/'));
    let (mut found, unreadable) = listed(host, |decision| decision.path.starts_with(&below))?;
    found.retain(|(_, listed)| listed.review == Review::Awaiting);
    found.sort_by(|(a, first), (b, second)| (a, &first.path).cmp(&(b, &second.path)));
    Ok(output(found, limit, unreadable))
}

pub(crate) fn about(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AboutInput { target, limit } = PluginError::read_input("decisions", "about", input)?;
    let target = target.trim_end_matches('/');
    let below = format!("{target}/");
    let (mut found, unreadable) = listed(host, |decision| {
        !decision.superseded
            && (decision.basis.iter()).any(|named| named == target || named.starts_with(&below))
    })?;
    found.sort_by(|(a, first), (b, second)| {
        (Reverse(a), &first.path).cmp(&(Reverse(b), &second.path))
    });
    Ok(output(found, limit, unreadable))
}

/// The readable decisions `keep` admits, each with its review state and
/// the instant it was decided, and how many decisions and questions were
/// unreadable.
fn listed(
    host: &dyn Host,
    keep: impl Fn(&Decision) -> bool,
) -> Result<(Vec<(board_plugin_plans::Instant, Listed)>, usize), PluginError> {
    let read = decisions(host, "decisions")?;
    let (mut reviews, unasked) = reviews(host)?;
    let listed = (read.decisions.into_iter().filter(keep)).map(|decision| {
        let Reviews { mut open, answered } = reviews.remove(&decision.path).unwrap_or_default();
        open.sort();
        let review = match (open.is_empty(), answered) {
            (false, _) => Review::Awaiting,
            (true, true) => Review::Reviewed,
            (true, false) => Review::Unreviewed,
        };
        let listed = Listed {
            path: decision.path,
            decision: decision.decision,
            why: decision.why,
            decided: decision.decided,
            basis: decision.basis,
            urls: decision.urls,
            review,
            questions: open,
            superseded: decision.superseded,
        };
        (decision.when, listed)
    });
    Ok((listed.collect(), read.unreadable + unasked))
}

/// What the readable questions say about each path their `for` names, and
/// how many questions were unreadable. Paths are compared as written, as
/// `supersedes` is (`crate::standing`).
fn reviews(host: &dyn Host) -> Result<(HashMap<String, Reviews>, usize), PluginError> {
    let (asked, unreadable) = rows(host, QUESTIONS, "decisions", |record| {
        Asked::deserialize(record).ok()
    })?;
    let mut reviews: HashMap<String, Reviews> = HashMap::new();
    for (question, asked) in asked {
        for named in asked.belongs_to {
            let review = reviews.entry(named).or_default();
            match asked.status {
                Status::Open => review.open.push(question.clone()),
                Status::Answered => review.answered = true,
                Status::Withdrawn => {}
            }
        }
    }
    Ok((reviews, unreadable))
}

fn output(
    found: Vec<(board_plugin_plans::Instant, Listed)>,
    limit: u16,
    unreadable: usize,
) -> Value {
    let (total, limit) = (found.len(), usize::from(limit));
    let output = Output {
        decisions: found
            .into_iter()
            .take(limit)
            .map(|(_, listed)| listed)
            .collect(),
        total,
        truncated: total > limit,
        unreadable,
    };
    serde_json::to_value(output).expect("Output holds only strings, numbers and bools")
}

#[cfg(test)]
mod tests;

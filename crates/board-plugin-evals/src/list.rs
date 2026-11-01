//! `about` and `open`: the evals this board and its mounted boards hold,
//! by path.

use std::collections::HashMap;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::{TREE_CODES, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::Verdict;

/// Every eval, by path. Each capability filters the rows in Rust:
/// `eq_ignore_ascii_case` compares as `SQLite`'s `lower` does, and
/// `starts_with` as `substr` and `length` do.
pub(crate) const EVALS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'evals/eval/v1' AND logical IS NOT NULL ORDER BY logical";

/// The evals that name this `subject` or this `repo`, at most `limit` of them.
///
/// A subject matches ignoring case for ASCII letters. A repo is a repo record's path such as `/repos/crumbkit`.
/// It matches an eval whose `repos` include that exact path, or whose own path lies under it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{"required": ["subject"]}, {"required": ["repo"]}]))]
pub(crate) struct AboutInput {
    /// The name of what is tested, such as crumbkit or clap, matched ignoring ASCII case; give subject or repo, not both.
    #[schemars(length(min = 1))]
    subject: Option<String>,
    /// A repository's repo record, such as /repos/crumbkit, to list the evals that name it or live in its mount; give subject or repo, not both.
    #[schemars(length(min = 1))]
    repo: Option<String>,
    /// The most evals to list, 1 to 100; 100 when left out.
    #[serde(default = "most", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 100))]
    limit: u16,
}

/// The evals still to do or being done, at most `limit` of them.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenInput {
    /// The most evals to list, 1 to 100; 100 when left out.
    #[serde(default = "most", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 100))]
    limit: u16,
}

/// The most evals a capability lists, and its default `limit`.
fn most() -> u16 {
    100
}

/// Evals by path. `total` counts every match, and `truncated` says whether the limit left some out.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListOutput")]
pub(crate) struct Output {
    evals: Vec<Listed>,
    total: usize,
    truncated: bool,
}

/// An eval, by its path in the board holding it, which may be a mounted one.
/// `decisions` are the decisions not superseded whose `basis` names it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Listed {
    path: String,
    question: String,
    subjects: Vec<String>,
    status: Status,
    verdict: Option<Verdict>,
    informs: Option<String>,
    decisions: Vec<String>,
}

/// Where an eval stands, as its fields derive it: `abandoned` once
/// `abandoned` is set, else `concluded` once it has a `verdict`, else
/// `running` once it lists `runs`, else `planned`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Status {
    Planned,
    Running,
    Concluded,
    Abandoned,
}

/// An eval as a mounted board's own, possibly looser, schema may admit it.
/// Null stands for a field left out, and an unknown field is ignored. The
/// fields named with `_` are read only to check their type.
#[derive(Deserialize)]
struct Stored {
    question: String,
    subjects: Vec<String>,
    verdict: Option<Verdict>,
    informs: Option<String>,
    written_by: Option<String>,
    repos: Option<Vec<String>>,
    runs: Option<Vec<String>>,
    abandoned: Option<String>,
    #[serde(rename = "conclusion")]
    _conclusion: Option<String>,
    #[serde(rename = "suites")]
    _suites: Option<Vec<String>>,
}

impl Stored {
    fn status(&self) -> Status {
        if self.abandoned.is_some() {
            Status::Abandoned
        } else if self.verdict.is_some() {
            Status::Concluded
        } else if self.runs.as_ref().is_some_and(|runs| !runs.is_empty()) {
            Status::Running
        } else {
            Status::Planned
        }
    }
}

pub(crate) fn about(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AboutInput {
        subject,
        repo,
        limit,
    } = PluginError::read_input("evals", "about", input)?;
    match (repo, subject) {
        (Some(repo), _) => {
            let below = format!("{repo}/");
            listed(host, limit, |path, eval| {
                eval.repos.iter().flatten().any(|named| *named == repo) || path.starts_with(&below)
            })
        }
        (None, subject) => {
            let subject = subject.unwrap_or_default();
            listed(host, limit, |_, eval| {
                (eval.subjects.iter()).any(|named| named.eq_ignore_ascii_case(&subject))
            })
        }
    }
}

pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let OpenInput { limit } = PluginError::read_input("evals", "open", input)?;
    listed(host, limit, |_, eval| {
        matches!(eval.status(), Status::Planned | Status::Running)
    })
}

/// The readable evals `keep` admits, by path, at most `limit` of them, each
/// with the decisions that rest on it. An eval Board marks degraded for
/// anything but the tree is left out, as is one this plugin cannot read.
fn listed(
    host: &dyn Host,
    limit: u16,
    keep: impl Fn(&str, &Stored) -> bool,
) -> Result<Value, PluginError> {
    let report = (host.query(EVALS, Limit::ALL))
        .map_err(|error| PluginError::board_refused("evals", "query", &error.refusal()))?;
    let mut resting: HashMap<String, Vec<String>> = HashMap::new();
    let standing = (board_plugin_decisions::standing::decisions(host, "evals")?
        .decisions
        .into_iter())
    .filter(|decision| !decision.superseded);
    for decision in standing {
        for named in decision.basis {
            resting
                .entry(named)
                .or_default()
                .push(decision.path.clone());
        }
    }
    let mut found = Vec::new();
    for row in &report.rows {
        let [Value::String(path), code, body] = &row[..] else {
            continue;
        };
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        if !tree {
            continue;
        }
        let Some(eval) = readable(body) else {
            continue;
        };
        if keep(path, &eval) {
            let mut decisions = resting.remove(path).unwrap_or_default();
            decisions.sort();
            decisions.dedup();
            let status = eval.status();
            found.push(Listed {
                path: path.clone(),
                question: eval.question,
                subjects: eval.subjects,
                status,
                verdict: eval.verdict,
                informs: eval.informs,
                decisions,
            });
        }
    }
    let (total, limit) = (found.len(), usize::from(limit));
    found.truncate(limit);
    let output = Output {
        evals: found,
        total,
        truncated: total > limit,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// `None` when `body` is not an eval this plugin reads: one with no
/// subject, or one whose `written_by` is not a plugin and hook.
fn readable(body: &Value) -> Option<Stored> {
    let record = parse_json(body.as_str()?.as_bytes()).ok()?;
    let eval = Stored::deserialize(&record).ok()?;
    let hook = |name: &str| {
        !name.is_empty()
            && (name.bytes()).all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    };
    let written = (eval.written_by.as_deref())
        .is_none_or(|by| by.split_once('/').is_some_and(|(a, b)| hook(a) && hook(b)));
    (!eval.subjects.is_empty() && written).then_some(eval)
}

#[cfg(test)]
mod tests;

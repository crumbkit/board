//! `about` and `open`: the evals this board and its mounted boards hold,
//! by path.

use std::collections::{BTreeMap, BTreeSet};

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::{Open, TREE_CODES, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::{Status, Verdict};

/// Every eval, by path. Each capability filters the rows in Rust:
/// `eq_ignore_ascii_case` compares as `SQLite`'s `lower` does, and
/// `starts_with` as `substr` and `length` do.
pub(crate) const EVALS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'evals/eval/v1' AND logical IS NOT NULL ORDER BY logical";

/// Every JSON topic, by the board that holds it and its path in that
/// board, which `about` reads to find the evals that name a topic whose
/// title or alias is the subject. The topics plugin ships the type.
pub(crate) const TOPICS: &str = "SELECT source, '/' || substr(path, 1, length(path) - 5), code, body \
     FROM records WHERE type = 'topics/topic/v1' AND path LIKE '%.json' ORDER BY source, path";

/// The board that holds each eval, so `about` matches a topic only in the
/// eval's own board.
pub(crate) const SOURCES: &str = "SELECT logical, source FROM records \
     WHERE type = 'evals/eval/v1' AND logical IS NOT NULL ORDER BY logical";

/// The evals that name this `subject` or this `mount`, at most `limit` of them.
///
/// A subject matches ignoring case for ASCII letters. A mount is a path such as `/repos/crumbkit`. It matches
/// an eval whose `mounts` include that exact path, or whose own path lies under it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{"required": ["subject"]}, {"required": ["mount"]}]))]
pub(crate) struct AboutInput {
    /// What is tested, as an eval's subjects name it, such as clap-rs/clap or clap alone, or as a topic's title or alias names it, such as seatbelt for an eval naming /topics/seatbelt, matched ignoring ASCII case; give subject or mount, not both.
    #[schemars(length(min = 1))]
    subject: Option<String>,
    /// A repository's mount, such as /repos/crumbkit, to list the evals that name it or live in it; give subject or mount, not both.
    #[schemars(length(min = 1))]
    mount: Option<String>,
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
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Listed {
    path: String,
    question: String,
    subjects: Vec<String>,
    /// planned, running, concluded or abandoned, or a value this Board does
    /// not know, as written.
    #[schemars(with = "String")]
    status: Open<Status>,
    /// supported, refuted or inconclusive, or a value this Board does not
    /// know, as written.
    #[schemars(with = "Option<String>")]
    verdict: Option<Open<Verdict>>,
    decision: Option<String>,
    informs: Option<String>,
}

/// An eval as a mounted board's own, possibly looser, schema may admit it.
/// Null stands for a field left out, and an unknown field is ignored. The
/// fields named with `_` are read only to check their type.
#[derive(Deserialize)]
struct Stored {
    question: String,
    subjects: Vec<String>,
    status: Open<Status>,
    verdict: Option<Open<Verdict>>,
    decision: Option<String>,
    informs: Option<String>,
    written_by: Option<String>,
    mounts: Option<Vec<String>>,
    #[serde(rename = "conclusion")]
    _conclusion: Option<String>,
    #[serde(rename = "suites")]
    _suites: Option<Vec<String>>,
    #[serde(rename = "runs")]
    _runs: Option<Vec<String>>,
}

pub(crate) fn about(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AboutInput {
        subject,
        mount,
        limit,
    } = PluginError::read_input("evals", "about", input)?;
    match (mount, subject) {
        (Some(mount), _) => {
            let below = format!("{mount}/");
            listed(host, limit, |path, eval| {
                eval.mounts.iter().flatten().any(|named| *named == mount)
                    || path.starts_with(&below)
            })
        }
        (None, subject) => {
            let subject = subject.unwrap_or_default();
            let topics = topics(host, &subject)?;
            let sources = if topics.is_empty() {
                BTreeMap::new()
            } else {
                sources(host)?
            };
            listed(host, limit, |path, eval| {
                let source = sources.get(path);
                (eval.subjects.iter()).any(|named| {
                    named.eq_ignore_ascii_case(&subject)
                        || repository_named(named, &subject)
                        || source
                            .is_some_and(|source| topics.contains(&(source.clone(), named.clone())))
                })
            })
        }
    }
}

/// The topics whose title or one of whose aliases is `subject`, ignoring
/// ASCII case, each as the source of the board that holds it, as JSON
/// text, and its path
/// in that board. A topic Board marks degraded for anything but the tree
/// is left out, as is one this plugin cannot read.
fn topics(host: &dyn Host, subject: &str) -> Result<BTreeSet<(String, String)>, PluginError> {
    /// A topic as far as `about` reads it; any other field is ignored.
    #[derive(Deserialize)]
    struct Topic {
        title: String,
        aliases: Option<Vec<String>>,
    }
    let report = (host.query(TOPICS, Limit::ALL))
        .map_err(|error| PluginError::board_refused("evals", "query", &error.refusal()))?;
    let found = (report.rows.iter()).filter_map(|row| {
        let [source, Value::String(path), code, Value::String(body)] = &row[..] else {
            return None;
        };
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        let topic = Topic::deserialize(parse_json(body.as_bytes()).ok()?).ok()?;
        let mut named = std::iter::once(&topic.title).chain(topic.aliases.iter().flatten());
        (tree && named.any(|name| name.eq_ignore_ascii_case(subject)))
            .then(|| (source.to_string(), path.clone()))
    });
    Ok(found.collect())
}

/// Whether the `owner/name` subject `named` is the repository `subject`
/// names by its name alone, such as `clap` for `clap-rs/clap`, ignoring
/// ASCII case. A board path never matches by its last part.
fn repository_named(named: &str, subject: &str) -> bool {
    !named.starts_with('/')
        && !subject.contains('/')
        && named
            .split_once('/')
            .is_some_and(|(_, name)| name.eq_ignore_ascii_case(subject))
}

/// The source of the board that holds each eval, as JSON text, by the
/// eval's path.
fn sources(host: &dyn Host) -> Result<BTreeMap<String, String>, PluginError> {
    let report = (host.query(SOURCES, Limit::ALL))
        .map_err(|error| PluginError::board_refused("evals", "query", &error.refusal()))?;
    Ok((report.rows.into_iter())
        .filter_map(|row| {
            let [Value::String(path), source] = &row[..] else {
                return None;
            };
            Some((path.clone(), source.to_string()))
        })
        .collect())
}

pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let OpenInput { limit } = PluginError::read_input("evals", "open", input)?;
    listed(host, limit, |_, eval| {
        !matches!(
            eval.status,
            Open::Known(Status::Concluded | Status::Abandoned)
        )
    })
}

/// The readable evals `keep` admits, by path, at most `limit` of them. An
/// eval Board marks degraded for anything but the tree is left out, as is
/// one this plugin cannot read.
fn listed(
    host: &dyn Host,
    limit: u16,
    keep: impl Fn(&str, &Stored) -> bool,
) -> Result<Value, PluginError> {
    let report = (host.query(EVALS, Limit::ALL))
        .map_err(|error| PluginError::board_refused("evals", "query", &error.refusal()))?;
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
            found.push(Listed {
                path: path.clone(),
                question: eval.question,
                subjects: eval.subjects,
                status: eval.status,
                verdict: eval.verdict,
                decision: eval.decision,
                informs: eval.informs,
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
/// subject, a concluded one without a verdict, or one whose `written_by`
/// is not a plugin and hook.
fn readable(body: &Value) -> Option<Stored> {
    let record = parse_json(body.as_str()?.as_bytes()).ok()?;
    let eval = Stored::deserialize(&record).ok()?;
    let concluded = eval.status == Open::Known(Status::Concluded) && eval.verdict.is_none();
    let hook = |name: &str| {
        !name.is_empty()
            && (name.bytes()).all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    };
    let written = (eval.written_by.as_deref())
        .is_none_or(|by| by.split_once('/').is_some_and(|(a, b)| hook(a) && hook(b)));
    (!eval.subjects.is_empty() && !concluded && written).then_some(eval)
}

#[cfg(test)]
mod tests;

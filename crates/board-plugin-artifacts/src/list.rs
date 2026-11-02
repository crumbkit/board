//! `artifacts.list`: the artifacts in the board with every version, newest
//! first, filtered by account, subject, state, area or what is unfiled.

use std::collections::BTreeSet;

use board_model::{Code, Refusal};
use board_ops::{Absent, Host, PluginError};
use board_plugin::shape::whole;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::held::{self, Artifacts, Held, Seen, logical, refused};
use crate::records::State;

/// The most artifacts one call returns, which is also the most the
/// Artifact tool lists.
pub(crate) const LIST_LIMIT: u16 = 200;

/// The artifacts in the board, newest first.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "ListInput")]
pub(crate) struct Input {
    /// Only the artifacts of this account, by its email address.
    #[schemars(length(min = 1))]
    account: Option<String>,
    /// Only the artifacts whose subject names this change or task, such as
    /// /changes/flows-editor.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    subject: Option<String>,
    /// Only the artifacts in this state.
    state: Option<State>,
    /// Only the artifacts with this area tag, such as dashboard/flows.
    #[schemars(length(min = 1))]
    area: Option<String>,
    /// true for only the artifacts with no subject or no shows.
    unfiled: bool,
    /// The most artifacts to return, 1 to 200; 50 when left out.
    #[serde(deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 200))]
    limit: u16,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            account: None,
            subject: None,
            state: None,
            area: None,
            unfiled: false,
            limit: 50,
        }
    }
}

/// Artifacts, newest first by their newest version, or by `first_seen` for
/// one with no version.
///
/// `total` counts every artifact that matched and `truncated` says whether
/// the limit left some out. `unreadable` counts the artifact and version
/// records Board marks degraded or this plugin cannot read, which were left
/// out.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListOutput")]
pub(crate) struct Output {
    artifacts: Vec<Listed>,
    total: usize,
    truncated: bool,
    unreadable: usize,
}

/// One artifact, by its record's path, with its fields and every version
/// the board holds, newest first.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Listed {
    path: String,
    url: String,
    account: String,
    first_seen: String,
    subject: Vec<String>,
    shows: Option<String>,
    state: State,
    agreed_by: Option<String>,
    superseded_by: Option<String>,
    area: Vec<String>,
    versions: Vec<Kept>,
}

/// One version, by its record's path. `version` is null for a version keyed
/// by its page's hash.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Kept {
    path: String,
    version: Option<String>,
    title: Option<String>,
    at: String,
}

/// Every artifact that matches `input`, newest first, at most its limit.
pub(crate) fn list(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input("artifacts", "list", input)?;
    let about = match &asked.subject {
        Some(subject) => Some(about(host, subject)?),
        None => None,
    };
    let Artifacts {
        held,
        mut versions,
        unreadable,
    } = held::all(host)?;
    let mut found: Vec<Listed> = (held.into_iter())
        .filter(|held| matches(&asked, about.as_ref(), held))
        .map(|held| {
            let seen = versions.remove(&held.path).unwrap_or_default();
            entry(held, seen)
        })
        .collect();
    found.sort_by(|a, b| recency(b).cmp(&recency(a)));
    let total = found.len();
    found.truncate(usize::from(asked.limit));
    let output = Output {
        truncated: total > found.len(),
        artifacts: found,
        total,
        unreadable,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings, numbers and lists"))
}

/// The artifacts whose `subject` reaches `subject`, by their paths, from
/// the records `board refs` finds pointing at it. A subject that holds
/// nothing has none, since nothing can link to it.
pub(crate) fn about(host: &dyn Host, subject: &str) -> Result<BTreeSet<String>, PluginError> {
    let not_found = |refusal: &Refusal| refusal.code.as_str() == Code::PathNotFound.as_str();
    let mut paths = BTreeSet::new();
    let answers = match host.refs(&[logical(subject)?], Absent::Refuse) {
        Ok(answers) => answers,
        Err(error) if not_found(&error.refusal()) => return Ok(paths),
        Err(error) => return Err(refused("refs", &error.refusal())),
    };
    for answer in answers {
        let report = match answer {
            Ok(report) => report,
            Err(error) if not_found(&error.refusal()) => continue,
            Err(error) => return Err(refused("refs", &error.refusal())),
        };
        for incoming in report.to {
            let from_subject =
                (incoming.pointers.iter()).any(|pointer| pointer.starts_with("/subject/"));
            if let (true, Some(path)) = (from_subject, incoming.logical) {
                paths.insert(path.as_str().to_owned());
            }
        }
    }
    Ok(paths)
}

fn matches(asked: &Input, about: Option<&BTreeSet<String>>, held: &Held) -> bool {
    let record = &held.record;
    (asked.account.as_ref()).is_none_or(|account| *account == record.account)
        && about.is_none_or(|paths| {
            paths.contains(&held.path)
                || (asked.subject.as_ref()).is_some_and(|subject| record.subject.contains(subject))
        })
        && asked.state.is_none_or(|state| state == record.state)
        && (asked.area.as_ref()).is_none_or(|area| record.area.contains(area))
        && (!asked.unfiled || record.subject.is_empty() || record.shows.is_none())
}

fn entry(held: Held, seen: Vec<Seen>) -> Listed {
    let Held { path, record } = held;
    Listed {
        path,
        url: record.url,
        account: record.account,
        first_seen: record.first_seen,
        subject: record.subject,
        shows: record.shows,
        state: record.state,
        agreed_by: record.agreed_by,
        superseded_by: record.superseded_by,
        area: record.area,
        versions: (seen.into_iter())
            .map(|Seen { path, record }| Kept {
                path,
                version: record.version,
                title: record.title,
                at: record.at,
            })
            .collect(),
    }
}

/// When the artifact last changed in the board: its newest version, or
/// when it was first seen, then its path. Times compare as text, which
/// orders the UTC times the mod and `import` write.
fn recency(entry: &Listed) -> (&str, &str) {
    let at =
        (entry.versions.first()).map_or(entry.first_seen.as_str(), |version| version.at.as_str());
    (at, entry.path.as_str())
}

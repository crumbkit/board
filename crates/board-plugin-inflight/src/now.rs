//! `inflight.now`: the open changes with their next step, pull request and
//! artifacts, the questions waiting on the owner and the open evals. It
//! writes nothing. Every part but the pull request comes from the owning
//! plugin's capability, so their rules stay in one place.

use std::collections::BTreeMap;

use board_ops::{Host, Limit, PluginError};
use board_plugin::Target;
use board_plugin::shape::whole;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What is open right now.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NowInput")]
pub(crate) struct Input {
    /// The most entries each part returns, and the most artifacts each
    /// change lists, 1 to 50; 20 when left out.
    #[serde(default = "most", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 50))]
    limit: u16,
}

fn most() -> u16 {
    20
}

/// The open changes, the questions waiting on the owner and the open
/// evals. Each part returns at most `limit` entries and counts in
/// `left_out` those it left out. When the capability a part comes from
/// refuses, the part is empty and `refused` says why.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NowOutput")]
pub(crate) struct Output {
    changes: Changes,
    questions: Waiting,
    evals: Waiting,
}

/// The entries of `changes.open`, by path. `unreadable` counts the changes
/// it left out because Board marks them degraded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Changes {
    changes: Vec<Change>,
    total: usize,
    left_out: usize,
    unreadable: usize,
    refused: Option<Refused>,
}

/// An open change as `changes.open` gives it, with `gate` and
/// `instruction` from `changes.next`, its pull request and its artifacts.
///
/// `pr_state` is the command that says whether the pull request merged,
/// since this query makes no network call. `refused` lists the calls for
/// this change that refused; `gate` and `instruction` are null when
/// `changes.next` did, and `artifacts` is empty when `artifacts.list` did.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Change {
    path: String,
    title: String,
    mount: String,
    phase: String,
    route: Option<String>,
    task: Option<String>,
    kind: String,
    gate: Option<Vec<Value>>,
    instruction: Option<String>,
    pull_request: Option<String>,
    pr_state: Option<String>,
    artifacts: Vec<Artifact>,
    artifacts_left_out: usize,
    refused: Vec<Refused>,
}

/// An artifact whose subject names the change, newest first, with the
/// title and time of its newest version, or null when it has none.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    url: String,
    state: String,
    title: Option<String>,
    at: Option<String>,
}

/// The entries of another plugin's capability, as it gives them.
/// `unreadable` is the count it gives of records it could not read, or 0
/// when it gives none.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Waiting {
    entries: Vec<Value>,
    total: usize,
    left_out: usize,
    unreadable: usize,
    refused: Option<Refused>,
}

/// A call that refused: the capability, and the code and message it
/// refused with.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Refused {
    capability: String,
    code: String,
    message: String,
}

#[derive(Deserialize)]
struct Opened {
    changes: Vec<OpenChange>,
    total: usize,
    unreadable: usize,
}

#[derive(Deserialize)]
struct OpenChange {
    path: String,
    title: String,
    mount: String,
    phase: String,
    route: Option<String>,
    task: Option<String>,
    kind: String,
}

#[derive(Deserialize)]
struct Next {
    gate: Vec<Value>,
    instruction: String,
}

#[derive(Deserialize)]
struct Listed {
    artifacts: Vec<ListedArtifact>,
    total: usize,
}

#[derive(Deserialize)]
struct ListedArtifact {
    path: String,
    url: String,
    state: String,
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Version {
    title: Option<String>,
    at: String,
}

#[derive(Deserialize)]
struct Entries {
    #[serde(alias = "questions", alias = "evals")]
    entries: Vec<Value>,
    total: usize,
    #[serde(default)]
    unreadable: usize,
}

pub(crate) fn now(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { limit } = PluginError::read_input("inflight", "now", input)?;
    let output = Output {
        changes: changes(host, limit)?,
        questions: waiting(host, "plans.open", limit),
        evals: waiting(host, "evals.open", limit),
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings, numbers and lists"))
}

fn changes(host: &dyn Host, limit: u16) -> Result<Changes, PluginError> {
    let opened = match call::<Opened>(host, "changes.open", &json!({"limit": limit})) {
        Ok(opened) => opened,
        Err(refused) => {
            return Ok(Changes {
                changes: Vec::new(),
                total: 0,
                left_out: 0,
                unreadable: 0,
                refused: Some(refused),
            });
        }
    };
    let pulls = pull_requests(host)?;
    let listed: Vec<Change> = (opened.changes.into_iter())
        .map(|open| change(host, open, &pulls, limit))
        .collect();
    Ok(Changes {
        left_out: opened.total.saturating_sub(listed.len()),
        changes: listed,
        total: opened.total,
        unreadable: opened.unreadable,
        refused: None,
    })
}

fn change(
    host: &dyn Host,
    open: OpenChange,
    pulls: &BTreeMap<String, String>,
    limit: u16,
) -> Change {
    let mut refused = Vec::new();
    let (gate, instruction) = match call::<Next>(host, "changes.next", &json!({"path": open.path}))
    {
        Ok(next) => (Some(next.gate), Some(next.instruction)),
        Err(refusal) => {
            refused.push(refusal);
            (None, None)
        }
    };
    let asked = json!({"subject": open.path, "limit": limit});
    let (artifacts, artifacts_left_out) = match call::<Listed>(host, "artifacts.list", &asked) {
        Ok(listed) => {
            let left_out = listed.total.saturating_sub(listed.artifacts.len());
            (
                listed.artifacts.into_iter().map(artifact).collect(),
                left_out,
            )
        }
        Err(refusal) => {
            refused.push(refusal);
            (Vec::new(), 0)
        }
    };
    let pull_request = pulls.get(&open.path).cloned();
    Change {
        pr_state: (pull_request.as_ref()).map(|url| format!("gh pr view {url} --json state")),
        pull_request,
        path: open.path,
        title: open.title,
        mount: open.mount,
        phase: open.phase,
        route: open.route,
        task: open.task,
        kind: open.kind,
        gate,
        instruction,
        artifacts,
        artifacts_left_out,
        refused,
    }
}

fn artifact(listed: ListedArtifact) -> Artifact {
    let newest = listed.versions.into_iter().next();
    Artifact {
        path: listed.path,
        url: listed.url,
        state: listed.state,
        title: newest.as_ref().and_then(|version| version.title.clone()),
        at: newest.map(|version| version.at),
    }
}

/// Each change's `pull_request` URL, by the change's path. changes gives it
/// through no capability, so it is read from the records.
fn pull_requests(host: &dyn Host) -> Result<BTreeMap<String, String>, PluginError> {
    let sql = "SELECT logical, json_extract(body, '$.pull_request') FROM records \
         WHERE type = 'changes/change/v1' AND json_extract(body, '$.pull_request') IS NOT NULL";
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("inflight", "query", &error.refusal()))?;
    Ok((report.rows.into_iter())
        .filter_map(|row| match row.as_slice() {
            [Value::String(path), Value::String(url)] => Some((path.clone(), url.clone())),
            _ => None,
        })
        .collect())
}

fn waiting(host: &dyn Host, capability: &str, limit: u16) -> Waiting {
    match call::<Entries>(host, capability, &json!({"limit": limit})) {
        Ok(given) => Waiting {
            left_out: given.total.saturating_sub(given.entries.len()),
            entries: given.entries,
            total: given.total,
            unreadable: given.unreadable,
            refused: None,
        },
        Err(refused) => Waiting {
            entries: Vec::new(),
            total: 0,
            left_out: 0,
            unreadable: 0,
            refused: Some(refused),
        },
    }
}

/// The output of `board call <capability>` read as `T`, or why it refused.
/// An output that does not read as `T` is refused with
/// `inflight::call::unreadable`, since the two plugins then disagree.
fn call<T: DeserializeOwned>(
    host: &dyn Host,
    capability: &str,
    input: &Value,
) -> Result<T, Refused> {
    let target: Target = (capability.parse())
        .expect("each capability inflight calls is a constant <plugin>.<capability>");
    let refused = |code: &str, message: String| Refused {
        capability: capability.to_owned(),
        code: code.to_owned(),
        message,
    };
    let report = (host.call(&target, input))
        .map_err(|refusal| refused(refusal.code.as_str(), refusal.message))?;
    T::deserialize(&report.output).map_err(|error| {
        let message = format!("{capability} gave output inflight cannot read: {error}; update Board so the plugins agree");
        refused("inflight::call::unreadable", message)
    })
}

#[cfg(test)]
mod tests;

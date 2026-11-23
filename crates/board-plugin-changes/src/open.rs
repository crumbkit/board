//! The `open` capability: the changes that are not done or dropped, and,
//! by kind, every prototype.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{readable, rows, text};
use crate::change::{
    LastSync, branch_of, change_types, kind_in, last_sync, parsed, prototypes, state_of, sync_reads,
};
use crate::flow::{FlowSummary, Flows};
use crate::owed::{OWED_DAYS, owing};
use crate::records::{ChangeRecord, Kind, PrototypeRecord, Route, State};
use crate::{PLUGIN, Warning, to_value};

/// The default `limit` (STANDARDS 23).
pub(crate) const OPEN_LIMIT: usize = 100;

/// The changes that are not done or dropped, by path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OpenInput")]
pub(crate) struct Input {
    #[schemars(
        description = "Only changes of this kind; prototype also lists the settled and abandoned \
                       ones."
    )]
    kind: Option<Kind>,
    #[serde(default = "open_limit")]
    #[schemars(
        range(min = 1, max = 10_000),
        description = "The most changes to return, 1 to 10,000; 100 when left out."
    )]
    limit: usize,
}

fn open_limit() -> usize {
    OPEN_LIMIT
}

/// What a prototype asks and found, and the branch its code is on.
#[derive(Debug, Serialize, JsonSchema)]
struct Prototype {
    question: Option<String>,
    state: Option<State>,
    verdict: Option<String>,
    verdict_artifact: Option<String>,
    branch: String,
}

/// A change that is not done or dropped, its task and kind, the name and
/// phases of the flow it follows, and, for a prototype, what it asks and
/// found. `flow` is left out for a change that follows no flow, such as a
/// prototype, and for one whose flow Board does not read.
#[derive(Debug, Serialize, JsonSchema)]
struct Listed {
    path: String,
    title: String,
    mount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    flow: Option<FlowSummary>,
    phase: String,
    route: Option<Route>,
    task: Option<String>,
    kind: Kind,
    prototype: Option<Prototype>,
}

/// Open changes by path.
///
/// `total` counts every open change and `truncated` says whether the limit
/// left some out. `unreadable` is how many were left out because Board
/// marks them degraded. `sync` is the day of the last sync, stale only when
/// some change records a pull request that sync reads. `warnings` holds
/// `changes::owed::rechecks` while a recently done change still owes a
/// check that does not block, which `changes.owed` lists.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "OpenOutput")]
pub(crate) struct Output {
    changes: Vec<Listed>,
    total: usize,
    truncated: bool,
    unreadable: usize,
    sync: LastSync,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<Warning>,
}

/// The warning that done changes still owe checks, when any does.
fn owed_warning(host: &dyn Host) -> Result<Option<Warning>, PluginError> {
    let (owed, _) = owing(host)?;
    if owed.is_empty() {
        return Ok(None);
    }
    Ok(Some(Warning {
        code: "changes::owed::rechecks",
        detail: format!(
            "{} changes done in the last {OWED_DAYS} days still owe a check asked after the \
             merge, such as the owner's rechecks; run `board changes owed` for each and the \
             questions to ask",
            owed.len()
        ),
    }))
}

fn listed(
    path: &str,
    change: ChangeRecord,
    flow: Option<FlowSummary>,
    prototype: Option<&PrototypeRecord>,
) -> Listed {
    let kind = if prototype.is_some() {
        Kind::Prototype
    } else {
        Kind::Build
    };
    let prototype = prototype.map(|found| Prototype {
        question: Some(found.question.clone()),
        state: Some(state_of(&change.phase)),
        verdict: found.verdict.clone(),
        verdict_artifact: found.verdict_artifact.clone(),
        branch: branch_of(path),
    });
    Listed {
        path: path.to_owned(),
        title: change.title,
        mount: change.mount,
        flow,
        phase: change.phase,
        route: change.route,
        task: change.task,
        kind,
        prototype,
    }
}

/// Every change whose phase is not final, by path, at most `limit`; with
/// `kind`, only those of that kind. With `kind` prototype, a settled or
/// abandoned prototype is listed too, since its verdict is what is looked
/// for.
pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { kind, limit } = PluginError::read_input(PLUGIN, "open", input)?;
    let unfinished = if kind == Some(Kind::Prototype) {
        String::new()
    } else {
        " AND json_extract(body, '$.phase') NOT IN ('done', 'dropped')".to_owned()
    };
    let sql = format!(
        "SELECT logical, code, body FROM records WHERE type IN ({}){unfinished} ORDER BY logical",
        change_types()
    );
    let prototypes = prototypes(host)?;
    let mut found = Vec::new();
    let mut unreadable = 0;
    let mut reads = false;
    let mut flows = Flows::default();
    for row in rows(host, &sql)? {
        let change = (readable(row.get(1))).then(|| parsed(row.get(2))).flatten();
        let (Some(path), Some(change)) = (text(row.first()), change) else {
            unreadable += 1;
            continue;
        };
        reads |= sync_reads(path, &change);
        // A prototype record Board does not read leaves the change
        // unreadable, since its kind is not known.
        let prototype = match prototypes.get(path) {
            Some(None) => {
                unreadable += 1;
                continue;
            }
            Some(Some(record)) => Some(record),
            None => None,
        };
        if kind.is_none_or(|kind| kind_in(&prototypes, path) == kind) {
            let flow = (flows.of_change(host, path, &change)).filter(|_| prototype.is_none());
            found.push(listed(path, change, flow, prototype));
        }
    }
    let total = found.len();
    found.truncate(limit);
    Ok(to_value(Output {
        changes: found,
        total,
        truncated: total > limit,
        unreadable,
        sync: last_sync(host, reads)?,
        warnings: owed_warning(host)?.into_iter().collect(),
    }))
}

#[cfg(test)]
mod tests;

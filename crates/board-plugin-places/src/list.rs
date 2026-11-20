//! `list`: every machine's joins, each with its change's title and phase
//! and the status of its workspace. mxv gives the status of this machine's
//! workspaces at read time; another machine's is unknown.

use board_model::LogicalPath;
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, Slug, TREE_CODES, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{Held, held, refusal, refused, warning};
use crate::machine;
use crate::mxv::{self, Listing, Workspace};
use crate::records::{CHANGE_TYPE, WorkspaceChange};

/// How many joins to list.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "ListInput")]
pub(crate) struct Input {
    /// The most joins to return, 1 to 10,000; 100 when left out.
    #[serde(deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10_000))]
    limit: u16,
    /// true to return every join, whatever the limit.
    all: bool,
    /// Only the joins of the machine with this label, such as quokka; every machine's when left out.
    #[schemars(with = "Option<Slug>")]
    machine: Option<String>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            limit: 100,
            all: false,
            machine: None,
        }
    }
}

/// This machine's label and the joins by path. `total` counts every join
/// this plugin reads, and `truncated` says whether the limit left some out.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListOutput")]
pub(crate) struct Output {
    machine: String,
    joins: Vec<Row>,
    total: usize,
    truncated: bool,
}

/// One join: its path, the machine's label, the workspace's name, the id
/// the join holds, when it was written, its change and the workspace's
/// status.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListedJoin")]
pub(crate) struct Row {
    path: String,
    machine: String,
    workspace: String,
    workspace_id: String,
    since: String,
    change: Listed,
    status: Status,
}

/// The change a join names, with its title and phase, each null when the
/// change cannot be read.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListedChange")]
pub(crate) struct Listed {
    path: String,
    title: Option<String>,
    phase: Option<String>,
}

/// What mxv says of a join's workspace now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    /// A live workspace of that name has the join's id.
    Live,
    /// A live workspace of that name has another id.
    Replaced,
    /// No live workspace has that name, and an archived one of that name
    /// has the join's id.
    Archived,
    /// No workspace, live or archived, has that name.
    Gone,
    /// mxv prints no id for the live workspace of that name, or the only
    /// workspaces of that name are archived with no id or another id, or
    /// the join is another machine's, whose mxv this one cannot ask.
    Unknown,
}

/// The status of a join holding `id`, given the live workspace of its
/// name, if any, and the archived workspaces of its name.
pub(crate) fn status(id: &str, live: Option<&Workspace>, archived: &[&Workspace]) -> Status {
    match live {
        Some(Workspace { id: None, .. }) => Status::Unknown,
        Some(Workspace { id: Some(live), .. }) if live == id => Status::Live,
        Some(_) => Status::Replaced,
        None if archived.is_empty() => Status::Gone,
        None if archived.iter().any(|each| each.id.as_deref() == Some(id)) => Status::Archived,
        None => Status::Unknown,
    }
}

/// The statement that reads every join under `/places/`, or under
/// `/places/<machine>/` when a label is given. A label is a slug, so it
/// holds no wildcard, and `literal` quotes it.
pub(crate) fn joins_sql(machine: Option<&str>) -> String {
    let prefix = machine.map_or_else(
        || "/places/".to_owned(),
        |machine| format!("/places/{machine}/"),
    );
    format!(
        "SELECT logical, code, body FROM records WHERE type = {} AND logical LIKE {} \
         ORDER BY logical",
        literal(WorkspaceChange::TYPE),
        literal(&format!("{prefix}%")),
    )
}

/// One join read from the board, with the machine and workspace its path
/// names.
pub(crate) struct Joined {
    path: String,
    machine: String,
    workspace: String,
    join: WorkspaceChange,
}

/// The joins in path order, with their statuses. Only this machine's
/// joins ask mxv; mxv runs only when one of them is shown.
pub(crate) fn list(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        limit,
        all,
        machine: only,
    } = PluginError::read_input("places", "list", input)?;
    let machine = machine::read(host)?;
    let report = (host.query(&joins_sql(only.as_deref()), Limit::ALL))
        .map_err(|error| refused("query", &error.refusal()))?;
    let mut joins: Vec<Joined> = Vec::new();
    for row in &report.rows {
        let [Value::String(logical), code, body] = &row[..] else {
            continue;
        };
        // A join Board marks degraded for its own content is left out; one
        // whose change is dangling or of another type is still a join.
        let whole = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        match read(logical, body).filter(|_| whole) {
            Some(joined) if only.as_ref().is_none_or(|only| *only == joined.machine) => {
                joins.push(joined);
            }
            Some(_) => {}
            None => warn(host, logical),
        }
    }
    let total = joins.len();
    let shown = if all {
        total
    } else {
        usize::from(limit).min(total)
    };
    joins.truncate(shown);
    let (live, archived) = if joins.iter().any(|joined| joined.machine == machine) {
        (
            mxv::list(host, Listing::Live)?,
            mxv::list(host, Listing::Archived)?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let changes = changes(host, &joins)?;
    let rows = (joins.into_iter().zip(changes))
        .map(|(joined, change)| {
            let named = |each: &&Workspace| each.name == joined.workspace;
            let status = if joined.machine == machine {
                let held: Vec<&Workspace> = archived.iter().filter(named).collect();
                status(&joined.join.workspace_id, live.iter().find(named), &held)
            } else {
                Status::Unknown
            };
            Row {
                path: joined.path,
                machine: joined.machine,
                workspace: joined.workspace,
                workspace_id: joined.join.workspace_id,
                since: joined.join.since,
                change,
                status,
            }
        })
        .collect();
    let output = Output {
        machine,
        joins: rows,
        total,
        truncated: shown < total,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings, numbers and lists"))
}

/// The join at `logical` with the machine and workspace that its path
/// `/places/<machine>/<workspace>/changes/<change-name>` names, or none
/// when the path or the join cannot be read.
fn read(logical: &str, body: &Value) -> Option<Joined> {
    let segments: Vec<&str> = logical.split('/').collect();
    let ["", "places", machine, workspace, "changes", _] = segments[..] else {
        return None;
    };
    let body: Value = match body {
        Value::String(text) => serde_json::from_str(text).ok()?,
        other => other.clone(),
    };
    let join = WorkspaceChange::deserialize(&body).ok()?;
    Some(Joined {
        path: logical.to_owned(),
        machine: machine.to_owned(),
        workspace: workspace.to_owned(),
        join,
    })
}

/// Says that the join at `logical` was left out.
fn warn(host: &dyn Host, logical: &str) {
    host.warn(&warning(&refusal(
        "places::record::unreadable",
        format!(
            "{logical} is no {} record this plugin reads, so list leaves it out; run \
             `board check {logical}` and fix it",
            WorkspaceChange::TYPE
        ),
        &[("path", logical)],
    )));
}

/// Each join's change with its title and phase, read in one call. A change
/// that cannot be read keeps null ones, with a warning.
fn changes(host: &dyn Host, joins: &[Joined]) -> Result<Vec<Listed>, PluginError> {
    let paths: Vec<Option<LogicalPath>> = (joins.iter())
        .map(|joined| joined.join.change.parse().ok())
        .collect();
    let readable: Vec<LogicalPath> = paths.iter().flatten().cloned().collect();
    let mut answers = (host.get_each(&readable))
        .map_err(|error| refused("get", &error.refusal("get")))?
        .into_iter();
    let mut listed = Vec::new();
    for (joined, path) in joins.iter().zip(paths) {
        let mut change = Listed {
            path: joined.join.change.clone(),
            title: None,
            phase: None,
        };
        if path.is_some() {
            match answers.next().map(|answer| answer.map(held)) {
                Some(Ok(Ok(Held::Record(record, _)))) if record["$type"] == CHANGE_TYPE => {
                    change.title = record["title"].as_str().map(str::to_owned);
                    change.phase = record["phase"].as_str().map(str::to_owned);
                }
                Some(Ok(Ok(Held::Record(record, _)))) => {
                    let held = record["$type"].as_str().unwrap_or("untyped");
                    wrong_type(host, &change.path, &format!("a {held} record"));
                }
                Some(Ok(Ok(Held::Other))) => {
                    wrong_type(
                        host,
                        &change.path,
                        "a directory or a file that is not a record",
                    );
                }
                Some(Ok(Err(error))) => host.warn(&warning(&error)),
                Some(Err(error)) => host.warn(&error.refusal("get")),
                _ => {}
            }
        }
        listed.push(change);
    }
    Ok(listed)
}

/// Says that the change a join names at `path` is `what`, so its title and
/// phase are null.
fn wrong_type(host: &dyn Host, path: &str, what: &str) {
    host.warn(&warning(&refusal(
        "places::change::wrong_type",
        format!(
            "{path} is {what}, not a {CHANGE_TYPE} record, so list prints no title or phase \
             for it"
        ),
        &[("change", path)],
    )));
}

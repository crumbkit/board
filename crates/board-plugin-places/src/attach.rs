//! `attach`: write the join of one workspace on this machine to one change.

use board_model::LogicalPath;
use board_ops::{Host, PluginError};
use board_plugin::shape::{Actor, Person, Prose, Record, SessionId, Slug, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, Held, logical, refusal};
use crate::mxv::{self, Listing, Workspace};
use crate::records::{CHANGE_TYPE, WorkspaceChange};
use crate::{join_path, machine};

/// The change a workspace works on, and who says so.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AttachInput")]
pub(crate) struct Input {
    /// The change's logical path, such as /changes/fix-login.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    change: String,
    /// The mxv workspace's name; the workspace that holds the caller's directory when left out.
    #[schemars(length(min = 1))]
    workspace: Option<String>,
    /// The session that attaches it, by its local_ id.
    #[schemars(with = "Option<SessionId>")]
    session: Option<String>,
    /// The program that attaches it, such as claude-code.
    #[schemars(with = "Option<Slug>")]
    program: Option<String>,
    /// The person who attaches it, such as ana.
    #[schemars(with = "Option<Person>")]
    person: Option<String>,
    /// The name the agent that attaches it was given.
    #[schemars(with = "Option<Prose>")]
    agent: Option<String>,
}

/// The join after the call: its path, the machine and workspace it names,
/// the change and the workspace's id, whether this call wrote it, and
/// whether it replaced a join of an earlier workspace of the same name.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AttachOutput")]
pub(crate) struct Output {
    path: String,
    machine: String,
    workspace: String,
    change: String,
    workspace_id: String,
    written: bool,
    replaced: bool,
}

/// Writes the join at `/places/<machine>/<workspace>/changes/<change-name>`
/// unless the same join is there.
pub(crate) fn attach(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        change,
        workspace,
        session,
        program,
        person,
        agent,
    } = PluginError::read_input("places", "attach", input)?;
    let machine = machine::read(host)?;
    let workspaces = mxv::list(host, Listing::Live)?;
    let found = found(host, &workspaces, workspace.as_deref())?;
    let Some(id) = found.id.clone() else {
        return Err(no_id(found));
    };
    let change = logical(&change)?;
    check_change(host, &change)?;
    let path = join_path(&machine, &found.name, &change)?;
    let workspace_ref = format!("/places/{machine}/{}", found.name);
    let record = WorkspaceChange {
        tag: Tag::default(),
        workspace: workspace_ref,
        change: change.to_string(),
        workspace_id: id.clone(),
        since: board_model::utc_seconds(host.settings().now),
        by: Actor::of(session, program, person, agent),
    };
    let (written, replaced) = match board::get(host, &path)? {
        Held::Nothing => {
            board::put(
                host,
                &[json!({"path": path.as_str(), "expect": "none", "record": record})],
            )?;
            (true, false)
        }
        Held::Other => return Err(unreadable(&path)),
        Held::Record(held, blob) => {
            let held = read_join(&path, &held)?;
            if held.change != record.change {
                return Err(taken(&path, &held.change, &record.change));
            }
            if held.workspace_id == id {
                (false, false)
            } else {
                board::put(
                    host,
                    &[json!({"path": path.as_str(), "expect": blob, "record": record})],
                )?;
                (true, true)
            }
        }
    };
    let output = Output {
        path: path.to_string(),
        machine,
        workspace: found.name.clone(),
        change: change.to_string(),
        workspace_id: id,
        written,
        replaced,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and booleans"))
}

/// The live workspace named `name`, or the one that holds the caller's
/// directory when no name is given.
pub(crate) fn found<'a>(
    host: &dyn Host,
    workspaces: &'a [Workspace],
    name: Option<&str>,
) -> Result<&'a Workspace, PluginError> {
    if let Some(name) = name {
        return (workspaces.iter())
            .find(|workspace| workspace.name == name)
            .ok_or_else(|| {
                refusal(
                    "places::workspace::not_found",
                    format!("no live mxv workspace is named {name}; run `mxv list` for the names"),
                    &[("workspace", name)],
                )
            });
    }
    let caller = host.settings().caller.as_deref();
    caller
        .and_then(|caller| mxv::holding(workspaces, caller))
        .ok_or_else(|| {
            let shown = caller.map_or_else(
                || "unknown".to_owned(),
                |caller| caller.display().to_string(),
            );
            refusal(
                "places::workspace::outside",
                format!(
                    "no mxv workspace holds the directory board was started in ({shown}); \
                     pass --workspace <name> or run board from inside the workspace"
                ),
                &[("directory", &shown)],
            )
        })
}

/// `places::workspace::no_id` for a workspace mxv prints with no id.
fn no_id(workspace: &Workspace) -> PluginError {
    let why = match &workspace.problem {
        Some(problem) => format!(
            "mxv cannot read the workspace's record ({problem}); fix that record first, then \
             run attach again"
        ),
        None => "the workspace's record predates ids; mxv writes the id on its next change to \
                 the workspace, after which attach can run again"
            .to_owned(),
    };
    refusal(
        "places::workspace::no_id",
        format!(
            "mxv prints no id for the workspace {}: {why}",
            workspace.name
        ),
        &[("workspace", &workspace.name)],
    )
}

/// Refuses `change` unless it is a change record.
fn check_change(host: &dyn Host, change: &LogicalPath) -> Result<(), PluginError> {
    let wrong = |what: &str| {
        refusal(
            "places::change::wrong_type",
            format!("{change} is {what}, not a {CHANGE_TYPE} record; pass a change's path"),
            &[("change", change.as_str())],
        )
    };
    match board::get(host, change)? {
        Held::Record(record, _) if record["$type"] == CHANGE_TYPE => Ok(()),
        Held::Record(record, _) => Err(wrong(&format!(
            "a {} record",
            record["$type"].as_str().unwrap_or("untyped")
        ))),
        Held::Other => Err(wrong("a directory or a file that is not a record")),
        Held::Nothing => Err(refusal(
            "places::change::not_found",
            format!(
                "nothing is at {change}; run `board changes open` for the changes' paths, or \
                 `board mount update` when the change is in a mounted board"
            ),
            &[("change", change.as_str())],
        )),
    }
}

/// The join held at `path`, or `places::record::unreadable`.
pub(crate) fn read_join(path: &LogicalPath, held: &Value) -> Result<WorkspaceChange, PluginError> {
    if held["$type"] != WorkspaceChange::TYPE {
        return Err(unreadable(path));
    }
    WorkspaceChange::deserialize(held).map_err(|_| unreadable(path))
}

pub(crate) fn unreadable(path: &LogicalPath) -> PluginError {
    refusal(
        "places::record::unreadable",
        format!(
            "{path} holds no {} record this plugin reads; run `board check {path}` and fix it",
            WorkspaceChange::TYPE
        ),
        &[("path", path.as_str())],
    )
}

fn taken(path: &LogicalPath, held: &str, given: &str) -> PluginError {
    refusal(
        "places::attach::taken",
        format!(
            "{path} already joins the workspace to {held}, another change with the same name as \
             {given}; detach it first or name another workspace"
        ),
        &[("path", path.as_str()), ("change", held)],
    )
}

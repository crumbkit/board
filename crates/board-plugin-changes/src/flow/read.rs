//! Choosing a change's flow, and reading the flows a board holds and the
//! copy each change keeps.
//!
//! A board's own flow lives at `/flows/<name>` and wins over a shipped flow
//! of the same name. `/flows/repos/<mount name>` and `/flows/default` are
//! links, `board/mount/v1` records with a `path`, that name a flow for one
//! repository and for every repository. Board follows a link, so it names a
//! flow the board holds; a link to nothing chooses no flow. `start` copies
//! the chosen flow to `/changes/<name>/flow`, and the gates read that copy,
//! so editing a board's flow never moves a change in flight.

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::{DEFAULT, Flow, shipped};
use crate::board::{body, logical, readable, refused, rows, text};
use crate::change::name_in;
use crate::records::ChangeRecord;
use crate::refusal;

/// Where a board keeps its own flows.
const FLOWS: &str = "/flows/";
/// The record type of a link.
const LINK_TYPE: &str = "board/mount/v1";

/// A flow's name and its phases in order, which `next` and `open` show.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct FlowSummary {
    pub(crate) name: String,
    pub(crate) phases: Vec<String>,
}

impl From<&Flow> for FlowSummary {
    fn from(flow: &Flow) -> Self {
        Self {
            name: flow.name.clone(),
            phases: flow.names(),
        }
    }
}

/// The flow at the logical `path` in the board, or none when nothing is
/// there. Anything there that is not a flow Board reads is
/// `changes::flow::unreadable`.
fn at_path(host: &dyn Host, path: &str) -> Result<Option<Flow>, PluginError> {
    let sql = format!(
        "SELECT e.type, e.code, b.body FROM entries AS e LEFT JOIN bodies AS b ON b.blob = e.blob \
         WHERE e.logical = {}",
        literal(path)
    );
    let found = rows(host, &sql)?;
    let Some(row) = found.first() else {
        return Ok(None);
    };
    let flow = (text(row.first()) == Some(Flow::TYPE) && readable(row.get(1)))
        .then(|| serde_json::from_value(Value::Object(body(row.get(2)))).ok())
        .flatten();
    flow.map(Some).ok_or_else(|| {
        refusal(
            "changes::flow::unreadable",
            format!(
                "{path} holds no changes/flow/v1 that Board reads; run `board check {path}` and \
                 fix it"
            ),
            [("path", json!(path))],
        )
    })
}

/// The flow called `name`: the board's own at `/flows/<name>`, else the
/// shipped one, else none.
pub(crate) fn named(host: &dyn Host, name: &str) -> Result<Option<Flow>, PluginError> {
    Ok(at_path(host, &format!("{FLOWS}{name}"))?.or_else(|| shipped(name)))
}

/// The flow `--flow` names: by name, or, for a name that starts with `/`,
/// the flow record at that path, such as a draft; else
/// `changes::flow::not_found`.
pub(crate) fn asked_for(host: &dyn Host, name: &str) -> Result<Flow, PluginError> {
    let found = if name.starts_with('/') {
        at_path(host, name)?
    } else {
        named(host, name)?
    };
    found.ok_or_else(|| not_found(name, "--flow"))
}

/// The blob of the file at the logical `path`, or `none` when nothing is
/// there, as a `put` line expects it.
pub(crate) fn blob_at(host: &dyn Host, path: &str) -> Result<String, PluginError> {
    let sql = format!("SELECT blob FROM entries WHERE logical = {}", literal(path));
    let found = rows(host, &sql)?;
    Ok((found.first().and_then(|row| text(row.first())))
        .unwrap_or("none")
        .to_owned())
}

/// The flow the link at `at` points to, or none when no link is there.
/// Board follows the link, so its target must be a flow the board holds; a
/// link to nothing is `changes::flow::not_found`, and anything else there
/// `changes::flow::unreadable`.
fn linked(host: &dyn Host, at: &str) -> Result<Option<Flow>, PluginError> {
    let sql = format!(
        "SELECT type FROM entries WHERE logical = {} AND kind = 'pointer'",
        literal(at)
    );
    let found = rows(host, &sql)?;
    if found.first().and_then(|row| text(row.first())) != Some(LINK_TYPE) {
        return Ok(None);
    }
    let asked = logical(at, "get")?;
    let answer = (host.get_each(&[asked]))
        .map_err(|error| refused("get", &error.refusal("get")))?
        .into_iter()
        .next();
    let report = match answer {
        Some(Ok(report)) => report,
        Some(Err(error)) => {
            return Err(refusal(
                "changes::flow::not_found",
                format!(
                    "the link at {at} names no flow the board holds ({error}); write a \
                     changes/flow/v1 at its target, or point the link at {FLOWS}<name> of a flow \
                     the board holds"
                ),
                [("path", json!(at))],
            ));
        }
        None => return Ok(None),
    };
    let display = report.display.to_string();
    let flow = (report.record.ok())
        .filter(|record| record.get("$type").and_then(Value::as_str) == Some(Flow::TYPE))
        .and_then(|record| serde_json::from_value(record).ok());
    flow.map(Some).ok_or_else(|| {
        refusal(
            "changes::flow::unreadable",
            format!(
                "the link at {at} names {display}, which holds no changes/flow/v1 that Board \
                 reads; run `board check {display}` and fix it, or point the link at a flow"
            ),
            [("path", json!(display))],
        )
    })
}

/// `changes::flow::not_found` for the flow `named`, which `by` chose.
fn not_found(named: &str, by: &str) -> PluginError {
    refusal(
        "changes::flow::not_found",
        format!(
            "{by} names the flow {named}, which neither the board holds at {FLOWS}<name> or at \
             that path nor the plugin ships; name development, small or flow-authoring, or write \
             the flow at {FLOWS}<name>"
        ),
        [("flow", json!(named))],
    )
}

/// The flow a new change on `mount` follows: the one `asked` names, else
/// the one `/flows/repos/<mount name>` links to, else the one
/// `/flows/default` links to, else the development flow, the board's own
/// or the shipped one.
pub(crate) fn chosen(
    host: &dyn Host,
    mount: &str,
    asked: Option<&str>,
) -> Result<Flow, PluginError> {
    if let Some(asked) = asked {
        return asked_for(host, asked);
    }
    let repo = name_in(mount);
    let mut links = Vec::new();
    if !repo.is_empty() {
        links.push(format!("{FLOWS}repos/{repo}"));
    }
    links.push(format!("{FLOWS}default"));
    for link in &links {
        if let Some(flow) = linked(host, link)? {
            return Ok(flow);
        }
    }
    asked_for(host, DEFAULT)
}

/// The `put` line that writes `flow` as the change at `path`'s copy, at
/// `<path>/flow`, over the blob `expect`.
pub(crate) fn copy_line(path: &str, expect: &str, flow: &Flow) -> (String, Value) {
    let at = format!("{path}/flow");
    let line = json!({"path": at, "expect": expect, "record": flow});
    (at, line)
}

/// The flow the change at `path` follows: the copy its `flow` names, the
/// shipped development flow for a v1 change, or none for a change that
/// follows no flow. A copy Board does not read is
/// `changes::flow::unreadable`.
pub(crate) fn of_change(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<Option<Flow>, PluginError> {
    if change.v1 {
        return Ok(Some(super::development()));
    }
    let Some(copy) = &change.flow else {
        return Ok(None);
    };
    at_path(host, copy)?.map(Some).ok_or_else(|| {
        refusal(
            "changes::flow::unreadable",
            format!(
                "the change at {path} names its flow at {copy}, and nothing is there; restore it \
                 with `board changes reflow --path {path} --flow {DEFAULT}`"
            ),
            [("path", json!(copy))],
        )
    })
}

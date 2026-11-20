//! What mxv says about this machine's workspaces: each one's name, path
//! and id, read from `mxv list --json`, and the workspace that holds a
//! directory.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use board_plugin::child::{Exited, ProcessError};
use serde_json::Value;

use crate::board::refusal;

/// How long one `mxv list` may take.
const SECONDS: u64 = 10;

/// The fields each row is asked for. mxv 0.3.0 and later print `id` only
/// when it is asked for. `problem` says why mxv cannot read a workspace's
/// record, which `places::workspace::no_id` names.
const FIELDS: &str = "name,id,path,problem";

/// The variables mxv reads to find its store, which Board passes on only
/// when a capability names them.
pub(crate) const ENV: [&str; 2] = ["MXV_ROOT", "XDG_STATE_HOME"];

/// The hint of `places::mxv::no_id`.
const UPGRADE: &str = "upgrade mxv to 0.3.0 or later, which prints each workspace's id";

/// One workspace as `mxv list` prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Workspace {
    pub(crate) name: String,
    pub(crate) path: String,
    /// Its id, or none when mxv prints null or a value that is not 32
    /// lowercase hex digits.
    pub(crate) id: Option<String>,
    /// mxv's `problem` text, when the row has one that is not empty.
    pub(crate) problem: Option<String>,
}

/// Which workspaces to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    Live,
    Archived,
}

/// The arguments of `mxv list` for `listing`.
pub(crate) fn arguments(listing: Listing) -> Vec<String> {
    let mut arguments = vec!["list"];
    if listing == Listing::Archived {
        arguments.push("--archived");
    }
    arguments.extend(["--all", "--json", "--fields", FIELDS]);
    arguments.into_iter().map(str::to_owned).collect()
}

/// Every workspace of `listing`, as mxv on the PATH a process plugin would
/// see lists it.
pub(crate) fn list(host: &dyn Host, listing: Listing) -> Result<Vec<Workspace>, PluginError> {
    let arguments = arguments(listing);
    let ran = host.run(&Run {
        program: "mxv",
        arguments: &arguments,
        directory: None,
        input: b"",
        deadline: Duration::from_secs(SECONDS),
        added: &[],
    });
    rows(&ran)
}

/// The rows of mxv's run, or why there are none.
pub(crate) fn rows(ran: &Result<Exited, ProcessError>) -> Result<Vec<Workspace>, PluginError> {
    let exited = match ran {
        Err(ProcessError::NotStarted { source }) if source.kind() == io::ErrorKind::NotFound => {
            return Err(failed("mxv is not on PATH".to_owned()));
        }
        Err(ProcessError::NotStarted { source }) => {
            return Err(failed(format!("mxv did not start ({source})")));
        }
        Err(ProcessError::Timeout { .. }) => {
            return Err(failed(format!("mxv ran past {SECONDS} s")));
        }
        Err(ProcessError::TooLarge { limit, .. }) => {
            return Err(failed(format!("mxv printed more than {limit} bytes")));
        }
        Ok(exited) => exited,
    };
    if exited.code != Some(0) {
        return Err(refused_by_mxv(exited));
    }
    let document: Value = serde_json::from_slice(&exited.stdout).unwrap_or(Value::Null);
    let (Some("mxv/list/v1"), Some(data)) =
        (document["schema"].as_str(), document["data"].as_array())
    else {
        return Err(failed("mxv printed no mxv/list/v1 document".to_owned()));
    };
    if document["truncated"] == true {
        return Err(refusal(
            "places::mxv::cut",
            "mxv left workspaces out of its listing even with --all, so a workspace's status \
             would be a guess"
                .to_owned(),
            &[],
        ));
    }
    data.iter().map(row).collect()
}

/// One row, or `places::mxv::no_id` when it has no `id` key, as an mxv
/// older than 0.3.0 prints it.
fn row(row: &Value) -> Result<Workspace, PluginError> {
    let (Some(name), Some(path)) = (row["name"].as_str(), row["path"].as_str()) else {
        return Err(failed(format!(
            "mxv printed a row with no name or path: {row}"
        )));
    };
    let Some(id) = row.get("id") else {
        return Err(no_id(&format!(
            "mxv printed the workspace {name} with no id, so it is older than 0.3.0"
        )));
    };
    let id = (id.as_str()).filter(|id| is_id(id)).map(str::to_owned);
    let problem = (row["problem"].as_str())
        .filter(|problem| !problem.is_empty())
        .map(str::to_owned);
    Ok(Workspace {
        name: name.to_owned(),
        path: path.to_owned(),
        id,
        problem,
    })
}

/// Whether `id` is 32 lowercase hex digits, as `WorkspaceId` admits.
pub(crate) fn is_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// mxv's refusal: `places::mxv::no_id` when it refused `--fields` for
/// naming `id`, as an mxv older than 0.3.0 does, and `places::mxv::failed`
/// otherwise, with mxv's message or the end of its stderr.
fn refused_by_mxv(exited: &Exited) -> PluginError {
    let failure = [&exited.stdout, &exited.stderr]
        .into_iter()
        .filter_map(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .find(|document| document["schema"] == "mxv/failure/v1");
    if let Some(failure) = &failure
        && failure["code"] == "mxv::cli::usage"
        && (failure["fields"]["value"] == "id"
            || (failure["message"].as_str()).is_some_and(|message| message.contains("'id'")))
    {
        return no_id(&format!(
            "mxv refused --fields {FIELDS}, so it is older than 0.3.0"
        ));
    }
    let code = (exited.code).map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
    let said = failure
        .and_then(|failure| failure["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            let stderr = String::from_utf8_lossy(&exited.stderr);
            (stderr.trim().lines().last().unwrap_or_default())
                .chars()
                .take(300)
                .collect()
        });
    failed(format!("mxv exited {code}: {said}"))
}

fn failed(message: String) -> PluginError {
    PluginError {
        hint: Some(format!(
            "run `mxv list --all --json --fields {FIELDS}` to see mxv's answer"
        )),
        ..refusal("places::mxv::failed", message, &[])
    }
}

fn no_id(message: &str) -> PluginError {
    PluginError {
        hint: Some(UPGRADE.to_owned()),
        ..refusal("places::mxv::no_id", format!("{message}; {UPGRADE}"), &[])
    }
}

/// The workspace whose directory holds `directory`, itself included, both
/// with symbolic links resolved. When workspaces nest, the innermost one
/// holds it. A workspace whose directory cannot be resolved holds nothing.
pub(crate) fn holding<'a>(workspaces: &'a [Workspace], directory: &Path) -> Option<&'a Workspace> {
    let directory = directory.canonicalize().ok()?;
    (workspaces.iter())
        .filter_map(|workspace| {
            let root: PathBuf = Path::new(&workspace.path).canonicalize().ok()?;
            directory
                .starts_with(&root)
                .then_some((root.components().count(), workspace))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, workspace)| workspace)
}

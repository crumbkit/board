//! pr-create's last step: attaching the pull request it opened to its
//! change, as `board changes attach --pull` does. It never fails the call,
//! since the pull request is already open.

use std::path::Path;

use board_model::LogicalPath;
use board_ops::{GetReport, Host};
use board_plugin::Target;
use serde_json::{Value, json};

use crate::crossing;
use crate::hook::quoted;
use crate::merge::own;

/// Why pr-create attached the pull request to no change, and the change
/// the attach command names: one named or read, or none when it found no
/// change it may attach to.
struct Missed {
    why: String,
    change: Option<String>,
}

/// Attaches the pull request recorded at `pull` to the change at `named`,
/// or, with none named, to the open change on the mount of the checkout at
/// `top` that is named after `branch`, in the board the command runs in.
/// Returns the change's path, or a note saying why it attached none.
pub(crate) fn attach(
    host: &dyn Host,
    top: &Path,
    named: Option<&str>,
    branch: &str,
    pull: &str,
) -> Result<String, String> {
    let path = named.map_or_else(|| format!("/changes/{branch}"), str::to_owned);
    let board = own(host).map_err(|error| note(None, &error.message, named, pull))?;
    let front = read(&*board, pull).map(|report| {
        format!(
            "board -C {}",
            quoted(&report.location.board.repository.display())
        )
    });
    attached(&*board, top, &path, named, pull)
        .map(|()| path.clone())
        .map_err(|missed| {
            note(
                front.as_deref(),
                &missed.why,
                missed.change.as_deref(),
                pull,
            )
        })
}

/// The note for the pull request at `pull` attached to no change because
/// of `why`, with the attach command for `change`, or for a change to
/// choose when none. `front` names the board the command runs in; without
/// one, the note says to run it in the board pr-create ran in.
fn note(front: Option<&str>, why: &str, change: Option<&str>, pull: &str) -> String {
    let to = change.map_or_else(|| "/changes/<name>".to_owned(), |change| quoted(&change));
    let (front, there) = front.map_or(
        (
            "board",
            " in the board this pr-create ran in, with the same `-C` or `--personal`",
        ),
        |front| (front, ""),
    );
    format!(
        "pr-create attached the pull request to no change: {why}; attach it with `{front} \
         changes attach --path {to} --pull {}`{there}",
        quoted(&pull)
    )
}

/// What `board` reads at `path`, or none when it reads nothing there.
fn read(board: &dyn Host, path: &str) -> Option<GetReport> {
    let logical = path.parse::<LogicalPath>().ok()?;
    board.get_each(&[logical]).ok()?.into_iter().next()?.ok()
}

/// Attaches `pull` to the change at `path` unless it already lists it.
/// A change not `named` was found by the branch's name, so it must be open
/// and on the checkout's mount.
fn attached(
    board: &dyn Host,
    top: &Path,
    path: &str,
    named: Option<&str>,
    pull: &str,
) -> Result<(), Missed> {
    let unnamed = |why| Missed {
        why,
        change: named.map(str::to_owned),
    };
    let record = (read(board, path).and_then(|report| report.record.ok())).filter(|record| {
        (record.get("$type").and_then(Value::as_str))
            .is_some_and(|kind| kind.starts_with("changes/change/"))
    });
    let Some(record) = record else {
        return Err(unnamed(format!("no change is at {path}")));
    };
    let text = |key| record.get(key).and_then(Value::as_str);
    if named.is_none() {
        if let Some(phase @ ("done" | "dropped")) = text("phase") {
            return Err(unnamed(format!("the change at {path} is {phase}")));
        }
        let mount = crossing::mount(board, top).map_err(|error| unnamed(error.message))?;
        if text("mount") != mount.as_deref() {
            return Err(unnamed(format!(
                "the change at {path} is on {}, not on this checkout's mount {}",
                text("mount").unwrap_or("no mount"),
                mount.as_deref().unwrap_or("(none)")
            )));
        }
    }
    let pulls = record.get("pulls").and_then(Value::as_array);
    if pulls.is_some_and(|pulls| pulls.iter().any(|each| each == pull)) {
        return Ok(());
    }
    let target: Target = "changes.attach"
        .parse()
        .expect("changes.attach is <plugin>.<capability>");
    (board.call(&target, &json!({"path": path, "pull": pull})))
        .map(|_| ())
        .map_err(|refusal| Missed {
            why: format!("{} ({})", refusal.message, refusal.code.as_str()),
            change: Some(path.to_owned()),
        })
}

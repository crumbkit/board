//! pr-create's last step: attaching the pull request it opened to its
//! change, as `board changes attach --pull` does. It never fails the call,
//! since the pull request is already open.

use std::path::Path;

use board_model::LogicalPath;
use board_ops::Host;
use board_plugin::Target;
use serde_json::{Value, json};

use crate::crossing;
use crate::merge::own;

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
    attached(host, top, &path, named.is_none(), pull)
        .map(|()| path.clone())
        .map_err(|why| {
            let to = if named.is_some() {
                &path
            } else {
                "/changes/<name>"
            };
            format!(
                "pr-create attached the pull request to no change: {why}; attach it with \
                 `board changes attach --path {to} --pull {pull}`"
            )
        })
}

/// Attaches `pull` to the change at `path` unless it already lists it.
/// When the change was `found` by the branch's name, it must be open and on
/// the checkout's mount.
fn attached(
    host: &dyn Host,
    top: &Path,
    path: &str,
    found: bool,
    pull: &str,
) -> Result<(), String> {
    let board = own(host).map_err(|error| error.message)?;
    let record = (path.parse::<LogicalPath>().ok())
        .and_then(|logical| board.get_each(&[logical]).ok()?.into_iter().next()?.ok())
        .and_then(|report| report.record.ok())
        .filter(|record| {
            (record.get("$type").and_then(Value::as_str))
                .is_some_and(|kind| kind.starts_with("changes/change/"))
        });
    let Some(record) = record else {
        return Err(format!("no change is at {path}"));
    };
    let text = |key| record.get(key).and_then(Value::as_str);
    if found {
        if let Some(phase @ ("done" | "dropped")) = text("phase") {
            return Err(format!("the change at {path} is {phase}"));
        }
        let mount = crossing::mount(&*board, top).map_err(|error| error.message)?;
        if text("mount") != mount.as_deref() {
            return Err(format!(
                "the change at {path} is on {}, not on this checkout's mount {}",
                text("mount").unwrap_or("no mount"),
                mount.as_deref().unwrap_or("(none)")
            ));
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
        .map_err(|refusal| format!("{} ({})", refusal.message, refusal.code.as_str()))
}

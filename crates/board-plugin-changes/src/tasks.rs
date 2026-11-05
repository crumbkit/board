//! A change's task: one the caller names, or one the tasks plugin adds.
//!
//! The task is written by a nested call of `tasks.add`, so the tasks
//! plugin's routing, sync and push apply, and this plugin never writes a
//! task file itself.

use board_ops::{Host, Origin, PluginError, literal};
use serde_json::{Map, Value, json};

use crate::board::{self, rows, text};
use crate::refusal;

/// Where a notes repo keeps its tasks, which splits a task's board path
/// into its repo and its path in that repo.
const TASKS_DIR: &str = "/tasks/";

/// A task's board path as its notes repo and its path in that repo.
///
/// `/repos/notes/tasks/a.md` is (`/repos/notes`, `/tasks/a.md`), and
/// `/tasks/a.md` is (`/`, `/tasks/a.md`).
pub(crate) fn split_task(path: &str) -> Result<(&str, &str), PluginError> {
    let Some(at) = path.rfind(TASKS_DIR) else {
        return Err(refusal(
            "changes::task::not_a_task_path",
            format!(
                "{path} is not under a tasks/ directory, so its notes repo is not known; name a \
                 task such as /repos/notes-personal/tasks/<slug>.md, and list them with `board \
                 call tasks.available`"
            ),
            [("path", json!(path))],
        ));
    };
    let repo = if at == 0 { "/" } else { &path[..at] };
    Ok((repo, &path[at..]))
}

/// The `tasks.add` input that sends a new task to `repo`, or else to the
/// repo the caller's origin routes to.
///
/// A nested call reads no caller of its own, so the caller's origin is
/// passed on as `remote`. A caller in a git repository whose origin git did
/// not give is refused, as `tasks.add` refuses it.
pub(crate) fn routing(
    host: &dyn Host,
    repo: Option<&str>,
) -> Result<Map<String, Value>, PluginError> {
    let mut route = Map::new();
    if let Some(repo) = repo {
        route.insert("repo".to_owned(), json!(repo));
        return Ok(route);
    }
    match host.caller_remote() {
        Origin::Url(remote) => {
            route.insert("remote".to_owned(), json!(remote));
        }
        Origin::Outside => {}
        Origin::Unread(why) => {
            return Err(refusal(
                "changes::task::unrouted",
                format!(
                    "{why}, so the notes repo for the change's task is not known; name an \
                     existing task with --task, or the repo to add one to with --task-repo"
                ),
                [("origin_unread", json!(why))],
            ));
        }
    }
    Ok(route)
}

/// Adds an open task titled `title` through `tasks.add`, sent where `route`
/// says, and returns its board path.
///
/// `parent` is the parent task's board path; the new task goes to the
/// parent's repo, since a task's parent is in its own repo.
pub(crate) fn add_task(
    host: &dyn Host,
    title: &str,
    mut route: Map<String, Value>,
    parent: Option<&str>,
) -> Result<String, PluginError> {
    route.insert("title".to_owned(), json!(title));
    if let Some(parent) = parent {
        let (repo, path) = split_task(parent)?;
        route.insert("repo".to_owned(), json!(repo));
        route.insert("parent".to_owned(), json!(path));
        route.remove("remote");
    }
    let output = board::call(host, "tasks.add", &Value::Object(route))?;
    Ok((output.get("path").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned())
}

/// Closes the open task at `path` through `tasks.done` when `settled`, or
/// `tasks.drop` when not.
///
/// A task already done or dropped, or one Board cannot read, is left as it
/// is.
pub(crate) fn close_task(host: &dyn Host, path: &str, settled: bool) -> Result<(), PluginError> {
    let sql = format!(
        "SELECT json_extract(body, '$.status') FROM records WHERE logical = {}",
        literal(path)
    );
    let found = rows(host, &sql)?;
    if found.first().and_then(|row| text(row.first())) != Some("open") {
        return Ok(());
    }
    let capability = if settled { "tasks.done" } else { "tasks.drop" };
    board::call(host, capability, &json!({"path": path}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::split_task;

    #[test]
    fn a_task_path_splits_into_its_repo_and_its_path_in_the_repo() {
        assert_eq!(
            split_task("/repos/notes/tasks/a.md").unwrap(),
            ("/repos/notes", "/tasks/a.md")
        );
        assert_eq!(split_task("/tasks/a.md").unwrap(), ("/", "/tasks/a.md"));
    }

    #[test]
    fn a_path_under_no_tasks_directory_is_refused() {
        let refused = split_task("/repos/notes/a.md").unwrap_err();
        assert_eq!(refused.code, "changes::task::not_a_task_path");
    }
}

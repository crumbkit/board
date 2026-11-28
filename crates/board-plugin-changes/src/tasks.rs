//! A change's task: one the caller names, or one the tasks plugin adds,
//! and the tasks its plan closes when its pull request merges.
//!
//! The task is written by a nested call of `tasks.add`, and closed by one of
//! `tasks.done` or `tasks.drop`, so the tasks plugin's routing, sync and
//! push apply, and this plugin never writes a task file itself.

use board_ops::{Host, Origin, PluginError, literal};
use serde_json::{Map, Value, json};

use crate::board::{self, get_text, rows, text};
use crate::change::{Referenced, attached, referenced};
use crate::gates::{FENCES, HEADING, Observed, open_task};
use crate::phases::plan_file;
use crate::records::{ChangeRecord, TASK_TYPE};
use crate::refusal;

/// The heading, in lower case, of the plan section that lists the tasks
/// the change closes when its pull request merges.
pub(crate) const CLOSES_AT_MERGE: &str = "closes at merge";

/// Where a notes repo keeps its tasks, which splits a task's board path
/// into its repo and its path in that repo.
const TASKS_DIR: &str = "/tasks/";

/// Why a change's task given the board's own tree belongs there, which
/// `tasks.add` needs for `/`.
const CHANGE_TASK: &str = "the task of a change, which belongs with the board that records it";

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
                 task such as /repos/notes/tasks/<slug>.md, and list them with `board \
                 tasks available`"
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
/// parent's repo, since a task's parent is in its own repo. A task sent
/// to the board's own tree carries [`CHANGE_TASK`] as its reason.
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
    if route.get("repo").and_then(Value::as_str) == Some("/") {
        route.insert("reason".to_owned(), json!(CHANGE_TASK));
    }
    let output = board::call(host, "tasks.add", &Value::Object(route))?;
    Ok((output.get("path").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned())
}

/// Closes the open task at `path` through `tasks.done` when `settled`, or
/// `tasks.drop` when not, and says whether it closed it.
///
/// A task already done or dropped, or one Board cannot read, is left as it
/// is.
pub(crate) fn close_task(host: &dyn Host, path: &str, settled: bool) -> Result<bool, PluginError> {
    if !is_open(host, path)? {
        return Ok(false);
    }
    let capability = if settled { "tasks.done" } else { "tasks.drop" };
    board::call(host, capability, &json!({"path": path}))?;
    Ok(true)
}

/// Whether the task at `path` has the status `open`. A task Board cannot
/// read is not.
fn is_open(host: &dyn Host, path: &str) -> Result<bool, PluginError> {
    let sql = format!(
        "SELECT json_extract(body, '$.status') FROM records WHERE logical = {}",
        literal(path)
    );
    let found = rows(host, &sql)?;
    Ok(found.first().and_then(|row| text(row.first())) == Some("open"))
}

/// The board paths the plan `text` names under its `## Closes at merge`
/// heading, each once, in order.
///
/// Only a level-two heading of that name counts; the section runs to the
/// next heading of level one or two, so it holds its subsections. Lines in
/// a fenced code block are not read, and a fenced line is never a heading.
/// A path is a whole word that starts with `/`, bare, in backticks or as a
/// Markdown link's target, so `/tasks/a.md` is not found inside
/// `/repos/n/tasks/a.md`. Punctuation around a word, such as the comma or
/// full stop after it, is not part of the path.
pub(crate) fn closes_at_merge(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut inside = false;
    let mut fenced = false;
    for line in text.lines() {
        if FENCES
            .iter()
            .any(|fence| line.trim_start().starts_with(fence))
        {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(heading) = HEADING.captures(line).and_then(|captures| captures.get(1)) {
            let level = line.len() - line.trim_start_matches('#').len();
            if level <= 2 {
                inside = level == 2 && heading.as_str().to_lowercase() == CLOSES_AT_MERGE;
            }
            continue;
        }
        if !inside {
            continue;
        }
        for word in line.split_whitespace() {
            let target = word.split_once("](").map_or(word, |(_, target)| target);
            let path = target
                .trim_start_matches(['`', '(', '<', '"', '\'', '*', '_'])
                .trim_end_matches(['`', ')', '>', '"', '\'', '*', '_', ',', '.', ';', ':']);
            if path.len() > 1 && path.starts_with('/') && !found.iter().any(|each| each == path) {
                found.push(path.to_owned());
            }
        }
    }
    found
}

/// The paths of `named` that are the change's task `own` or a task
/// `records` holds, and those that are neither, each in order.
pub(crate) fn attached_or_not<'a>(
    named: &'a [String],
    records: &Referenced,
    own: Option<&str>,
) -> (Vec<&'a str>, Vec<&'a str>) {
    (named.iter().map(String::as_str)).partition(|path| {
        Some(*path) == own || records.of_type(TASK_TYPE).any(|(task, _)| task == *path)
    })
}

/// What a change's merge does with its tasks: the tasks its plan closes at
/// merge, and the open attached tasks the plan does not name.
#[derive(Debug, Default)]
pub(crate) struct AtMerge {
    /// The named tasks that are the change's own or attached to it and
    /// still open, in the plan's order, which close when it merges.
    pub(crate) closes: Vec<String>,
    /// The attached tasks that are neither done nor dropped and that the
    /// plan does not name, which someone must close first.
    pub(crate) owed: Vec<String>,
}

/// What the merge of the change at `path` does with its tasks, read from
/// its current plan's text and the records attached to its plans.
pub(crate) fn at_merge(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<AtMerge, PluginError> {
    let records = referenced(host, &attached(host, path)?)?;
    let text = match plan_file(host, change)? {
        Some(file) => get_text(host, &file)?,
        None => None,
    };
    let named = closes_at_merge(text.as_deref().unwrap_or_default());
    let (attached_named, _) = attached_or_not(&named, &records, change.task.as_deref());
    let mut closes = Vec::new();
    for task in attached_named {
        if is_open(host, task)? {
            closes.push(task.to_owned());
        }
    }
    let owed = (records.of_type(TASK_TYPE))
        .filter(|(task, body)| open_task(body) && !named.iter().any(|each| each == task))
        .map(|(task, _)| task.to_owned())
        .collect();
    Ok(AtMerge { closes, owed })
}

/// Refuses to move the change at `path` from `phase` into done while
/// `tasks` lists an attached task that is still open and that the plan does
/// not close at merge, as `sync` leaves such a change owed. A plan whose
/// text Board cannot read names no task, so every open attached task is
/// such a task.
pub(crate) fn refuse_owed(path: &str, phase: &str, tasks: &AtMerge) -> Result<(), PluginError> {
    if tasks.owed.is_empty() {
        return Ok(());
    }
    Err(refusal(
        "changes::land::tasks_open",
        format!(
            "the change at {path} stays in {phase}: these tasks attached to it are not done or \
             dropped, and its plan does not name them under `## Closes at merge`: {}; close each \
             with `board tasks done` or `board tasks drop`, or name it under `## Closes at \
             merge` in the plan, then run `board changes advance --path {path}` again",
            tasks.owed.join(", ")
        ),
        [("path", json!(path)), ("tasks", json!(tasks.owed))],
    ))
}

/// Refuses to move the change at `path` from `phase` into done with no
/// pull request while a task its plan closes at merge is still open. No
/// merge closes those tasks, and Board closes a task only once GitHub
/// reports a merge, so moving on would leave each open with no word.
pub(crate) fn refuse_unmerged(path: &str, phase: &str, tasks: &AtMerge) -> Result<(), PluginError> {
    if tasks.closes.is_empty() {
        return Ok(());
    }
    Err(refusal(
        "changes::land::no_merge",
        format!(
            "the change at {path} stays in {phase}: it records no pull request, so no merge \
             closes these tasks its plan names under `## Closes at merge`, and Board closes a \
             task only once GitHub reports a merge: {}; record the pull request with `board \
             github ref` and attach it with `board changes attach --path {path} --pull \
             <record>`, or ask the owner to close each task with `board tasks done`, then run \
             `board changes advance --path {path}` again",
            tasks.closes.join(", ")
        ),
        [("path", json!(path)), ("tasks", json!(tasks.closes))],
    ))
}

/// Closes each of `named` still open through `tasks.done`, in order, and
/// returns those it closed. A task already done or dropped is skipped, so a
/// rerun after a refusal closes only the rest.
///
/// When `tasks.done` refuses a task, the change at `path` stays in `phase`
/// and the refusal is `changes::land::tasks_not_closed`, naming the task
/// and why; the tasks closed before it stay closed.
pub(crate) fn close_at_merge(
    host: &dyn Host,
    path: &str,
    phase: &str,
    named: &[String],
    rerun: &str,
) -> Result<Vec<String>, PluginError> {
    let mut closed = Vec::new();
    for task in named {
        match close_task(host, task, true) {
            Ok(true) => closed.push(task.clone()),
            Ok(false) => {}
            Err(error) => {
                return Err(refusal(
                    "changes::land::tasks_not_closed",
                    format!(
                        "the change at {path} stays in {phase}: its pull request merged, but \
                         closing {task}, which its plan closes at merge, was refused: {}; fix \
                         that, then run `{rerun}` again",
                        error.message
                    ),
                    [
                        ("path", json!(path)),
                        ("task", json!(task)),
                        ("refusal", json!(error.code)),
                    ],
                ));
            }
        }
    }
    Ok(closed)
}

/// Every attached task is done or dropped, except the change's own and
/// those the plan's Closes at merge section names, which close when the
/// change's pull request merges. Each path that section names is the
/// change's own task or an attached one.
pub(crate) fn tasks_missing(observed: &Observed) -> Option<String> {
    let own = observed.change.task.as_deref();
    let named = closes_at_merge(observed.plan.as_deref().unwrap_or_default());
    let (_, unattached) = attached_or_not(&named, &observed.records, own);
    if !unattached.is_empty() {
        return Some(format!(
            "the plan's Closes at merge section names {}, which the change neither has as its \
             task nor has attached as a task; attach each with `board changes attach --path {} \
             --ref <task>`, or remove it from the section",
            unattached.join(", "),
            observed.path
        ));
    }
    let left: Vec<&str> = (observed.records.of_type(TASK_TYPE))
        .filter(|(path, body)| {
            Some(*path) != own && open_task(body) && !named.iter().any(|each| each == path)
        })
        .map(|(path, _)| path)
        .collect();
    (!left.is_empty()).then(|| format!("these tasks are not done or dropped: {}", left.join(", ")))
}

/// What a passing `tasks` check says: the tasks still open that close at
/// merge, when the plan names any. A named task the change has attached
/// and that is done or dropped is left out.
pub(crate) fn tasks_passed(observed: &Observed) -> String {
    let closed: Vec<&str> = (observed.records.of_type(TASK_TYPE))
        .filter(|(_, body)| !open_task(body))
        .map(|(path, _)| path)
        .collect();
    let named: Vec<String> = closes_at_merge(observed.plan.as_deref().unwrap_or_default())
        .into_iter()
        .filter(|path| !closed.contains(&path.as_str()))
        .collect();
    if named.is_empty() {
        return String::new();
    }
    format!(
        "these tasks close when the pull request merges, as the plan's Closes at merge section \
         says: {}",
        named.join(", ")
    )
}

#[cfg(test)]
mod tests;

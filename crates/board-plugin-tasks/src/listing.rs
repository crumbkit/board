//! The tasks a call reads: the notes repos and the task files in one
//! query, each file read as a task, and what each task's `after` and
//! `parent` reach through one `refs`.

use std::collections::BTreeMap;

use board_model::{LogicalPath, Source};
use board_ops::{Absent, Host, Limit, PluginError, RefsReport};
use board_plugin::shape::{Record as _, TREE_CODES};
use board_repos::{COLUMNS, Date, Listed, json_body, parsed, query, root, shown};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::PLUGIN;
use crate::dates::Defer;
use crate::records::TaskRecord;
use crate::task::model;
use crate::tree::{Forest, Node, Nodes, forest, waits_on};

/// What the query reads of each Markdown file under a `tasks/` directory
/// of its repo that may be a task: its repo's root, its path, its Board
/// code, and its frontmatter as JSON when it is a `tasks/task/v2` record
/// that parses, or null.
const TASK_COLUMNS: [&str; 4] = [
    "substr(e.logical, 1, length(e.logical) - length(e.path))",
    "e.logical",
    "e.code",
    "b.body",
];

/// The notes repos and the task files in one query. The task rows are
/// padded with nulls to the repos' width.
fn listing_query() -> String {
    let padding = ", NULL".repeat(COLUMNS.len() - TASK_COLUMNS.len());
    format!(
        "SELECT 'repo', * FROM ({})
UNION ALL
SELECT 'task', *{padding} FROM (
SELECT {}
FROM entries AS e LEFT JOIN bodies AS b ON b.blob = e.blob AND e.type = 'tasks/task/v2'
WHERE e.path GLOB '*.md' AND ('/' || e.path) GLOB '*/tasks/*'
ORDER BY e.logical
)",
        query(),
        TASK_COLUMNS.join(", ")
    )
}

/// A task file's row: its repo's root, its logical path, its code and its
/// body.
pub(crate) type Row = [Value; 4];

/// The notes repos and the task files, from one query.
pub(crate) fn listing(host: &dyn Host) -> Result<(Vec<Listed>, Vec<Row>), PluginError> {
    let report = (host.query(&listing_query(), Limit::ALL))
        .map_err(|error| PluginError::board_refused(PLUGIN, "query", &error.refusal()))?;
    let repos: Vec<&[Value]> = (report.rows.iter())
        .filter(|row| row[0] == "repo")
        .map(|row| &row[1..=COLUMNS.len()])
        .collect();
    let found = parsed(PLUGIN, &repos)?;
    let rows = (report.rows.iter())
        .filter(|row| row[0] == "task")
        .map(|row| {
            [
                row[1].clone(),
                row[2].clone(),
                row[3].clone(),
                row[4].clone(),
            ]
        })
        .collect();
    Ok((found, rows))
}

/// A task, with the repo that holds it. `defer` keeps the text the
/// frontmatter wrote. `waits` and `linked` say whether it has an `after`
/// and a `parent`, which `refs` resolves.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Task {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) due: Option<Date>,
    pub(crate) defer: Option<(String, Defer)>,
    pub(crate) from_hour: Option<u8>,
    pub(crate) reviewed: Option<Date>,
    pub(crate) contexts: Vec<String>,
    pub(crate) waits: bool,
    pub(crate) owner: Option<String>,
    pub(crate) status: String,
    pub(crate) linked: bool,
    pub(crate) kind: String,
    pub(crate) brief: Option<String>,
}

/// The tasks in `roots`, in every status, and how many of their files are
/// degraded or hold a value the model cannot read. A task file is degraded
/// whatever its status when it is no `tasks/task/v2` record, when check
/// degrades it for more than the tree, or when the model refuses a record
/// the board's schema admitted. A date-time without an offset is local
/// time, `offset` seconds east of UTC.
pub(crate) fn read(rows: &[Row], roots: &[String], offset: i32) -> (Vec<Task>, usize) {
    let mut tasks = Vec::new();
    let mut degraded = 0;
    for [repo, path, code, body] in rows {
        let at = root(&shown(repo));
        if !roots.contains(&at) {
            continue;
        }
        let tree_only = code.as_str().is_none_or(|code| TREE_CODES.contains(&code));
        let path = shown(path);
        let record = (!body.is_null() && tree_only)
            .then(|| json_body(PLUGIN, &path, body).ok())
            .flatten();
        let Some(task) = record.and_then(|record| model(&record, offset).ok()) else {
            degraded += 1;
            continue;
        };
        let brief = match task.brief {
            Some(brief) if at != "/" => Some(format!("{at}{brief}")),
            brief => brief,
        };
        tasks.push(Task {
            path,
            title: task.title,
            due: task.due,
            defer: task.defer,
            from_hour: task.from_hour,
            reviewed: task.reviewed,
            contexts: task.contexts,
            waits: !task.after.is_empty(),
            owner: task.owner,
            status: task.status,
            linked: task.parent.is_some(),
            kind: task.kind.unwrap_or_else(|| "task".to_owned()),
            brief,
            repo: at,
        });
    }
    (tasks, degraded)
}

/// The root of the repo that holds the board path `path`: the mount whose
/// root is its longest prefix, else `/`.
pub(crate) fn held_by(found: &[Listed], path: &str) -> String {
    (found.iter())
        .map(|listed| &listed.root)
        .filter(|at| path.starts_with(&format!("{}/", at.trim_end_matches('/'))))
        .max_by_key(|at| at.len())
        .map_or_else(|| "/".to_owned(), Clone::clone)
}

/// An `after` that names a task in another notes repo than its task's
/// own, which is not followed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct CrossRepo {
    pub(crate) path: String,
    pub(crate) after: String,
}

/// A `parent` that names a task in another notes repo than its task's
/// own, which is not followed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct CrossParent {
    pub(crate) path: String,
    pub(crate) parent: String,
}

/// A reference to another notes repo's task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum Crossing {
    After(CrossRepo),
    Parent(CrossParent),
}

/// What `refs` resolved for tasks: each one's open `after` targets and
/// parent, by its path, and the crossings.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Links {
    pub(crate) blocked: BTreeMap<String, Vec<String>>,
    pub(crate) parents: BTreeMap<String, String>,
    pub(crate) crossing: Vec<Crossing>,
}

/// What each task with an `after` waits on and each task's parent, by the
/// task's path, and those not followed. One `refs` answers every task
/// with an `after` or a `parent`, in every status; none runs when no task
/// has one. An `after` or a `parent` that reaches a task in another notes
/// repo than its own, such as a task in the board's own tree naming one in
/// a mount, is listed and not followed.
pub(crate) fn resolve(host: &dyn Host, tasks: &[Task]) -> Result<Links, PluginError> {
    let asked: Vec<&Task> = (tasks.iter())
        .filter(|task| task.waits || task.linked)
        .collect();
    let mut links = Links::default();
    if asked.is_empty() {
        return Ok(links);
    }
    let paths: Vec<LogicalPath> = (asked.iter())
        .filter_map(|task| task.path.parse().ok())
        .collect();
    let fields = ["/status".to_owned(), "/$type".to_owned()];
    let reports = (host.refs(&paths, Absent::Refuse, &fields))
        .map_err(|error| PluginError::board_refused(PLUGIN, "refs", &error.refusal()))?;
    for (task, report) in asked.iter().zip(reports) {
        let Ok(report) = report else { continue };
        let (found, parent, across) = blockers(&report, &task.repo);
        if task.waits {
            links.blocked.insert(task.path.clone(), found);
        }
        if let Some(parent) = parent {
            links.parents.insert(task.path.clone(), parent);
        }
        links
            .crossing
            .extend(across.into_iter().map(|(after, target)| {
                let path = task.path.clone();
                if after {
                    Crossing::After(CrossRepo {
                        path,
                        after: target,
                    })
                } else {
                    Crossing::Parent(CrossParent {
                        path,
                        parent: target,
                    })
                }
            }));
    }
    Ok(links)
}

/// The tasks a task in `repo` reaches through `after` that are not done,
/// its parent, and the targets elsewhere, each with whether it is an
/// `after`. Each blocker is listed once, by its path in the board, and an
/// unresolved one as written; so is the parent. A target is done when it
/// is a `tasks/task/v2` record whose status is `done` and which check
/// degrades for nothing but the tree. A target held by another commit than
/// the task, or the board's own tree for a mount, is in another repo. Board
/// displays a target in a mount by its path in that mount, so the repo's
/// root is put in front.
fn blockers(report: &RefsReport, repo: &str) -> (Vec<String>, Option<String>, Vec<(bool, String)>) {
    let source: &Source = &report.location.board.source;
    let (mut found, mut parent, mut across) = (Vec::new(), None, Vec::new());
    for outgoing in &report.from {
        let after = outgoing.pointer.starts_with("/after/");
        if outgoing.link || !(after || outgoing.pointer == "/parent") {
            continue;
        }
        let target = outgoing.target.to_string();
        if let Some(location) = &outgoing.location
            && location.board.source != *source
        {
            across.push((after, target));
            continue;
        }
        let display = (outgoing.location.as_ref())
            .and_then(board_model::Location::display_path)
            .map_or(target, |path| path.to_string());
        let display = if repo == "/" {
            display
        } else {
            format!("{repo}{display}")
        };
        if !after {
            parent = Some(display);
            continue;
        }
        let fielded = outgoing.fields.as_ref();
        let value = |pointer: &str| {
            let values = fielded.and_then(|fielded| fielded.values.as_ref());
            (values.into_iter().flatten())
                .find(|(asked, _)| asked == pointer)
                .and_then(|(_, value)| value.as_ref())
        };
        let code = fielded.and_then(|fielded| fielded.code);
        let done = value("/$type").and_then(Value::as_str) == Some(TaskRecord::TYPE)
            && value("/status").and_then(Value::as_str) == Some("done")
            && code.is_none_or(|code| TREE_CODES.contains(&code.as_str()));
        if !done && !found.contains(&display) {
            found.push(display);
        }
    }
    (found, parent, across)
}

/// The tasks as the tree's nodes, by path, and the forest their parents
/// make.
pub(crate) fn grown(tasks: &[Task], links: &Links) -> (Nodes, Forest) {
    let nodes: Nodes = (tasks.iter())
        .map(|task| {
            let node = Node {
                title: task.title.clone(),
                status: task.status.clone(),
                parent: links.parents.get(&task.path).cloned(),
            };
            (task.path.clone(), node)
        })
        .collect();
    let tree = forest(&nodes);
    (nodes, tree)
}

/// What a task waits on: its open `after` targets, then the open
/// descendants nearest to it.
pub(crate) fn waited(links: &Links, tree: &Forest, nodes: &Nodes, path: &str) -> Vec<String> {
    let mut after = links.blocked.get(path).cloned().unwrap_or_default();
    let more: Vec<String> = (waits_on(tree, nodes, path).into_iter())
        .filter(|child| !after.contains(child))
        .collect();
    after.extend(more);
    after
}

#[cfg(test)]
mod tests;

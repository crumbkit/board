//! `available`: the open tasks to sweep now, those due soon, and the stale
//! ones (plugins/tasks/README.md).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use board_model::Code;
use board_ops::{Elsewhere, Host, PluginError};
use board_plugin::shape::{Actor, Person, SessionId, Slug};
use board_repos::{Date, Repo, Through, day_start, scope, today};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::dates::{Defer, Until};
use crate::listing::{Crossing, Task, grown, listing, read, resolve, waited};
use crate::task::actor;
use crate::tree::{Forest, Nodes, Subtasks, subtasks, tree_path};
use crate::{PLUGIN, chosen_by, writer};

/// The days ahead a due task is listed in `due_soon`.
const DUE_SOON: i64 = 7;
/// The days after which a task not reviewed is stale.
const STALE: i64 = 14;
/// The most tasks a section lists; `total` counts the rest (STANDARDS 23).
pub(crate) const SHOWN: usize = 100;
/// How long one repo's sync may take. A sync still running then is
/// stopped, and the repo is read at its pin.
const SYNC_DEADLINE: Duration = Duration::from_secs(5);
const SYNC_TIMEOUT: &str = "tasks::repo::sync_timeout";

/// The tasks to sweep now, in the repos the caller's remote chooses.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AvailableInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = r"^[a-z0-9-]+$"),
        description = "Only tasks tagged with this context, such as errands."
    )]
    pub(crate) context: Option<String>,
    #[schemars(
        with = "Option<SessionId>",
        description = "With program and person, the owner: only tasks it holds or nobody holds, \
                       and in held the ones it holds that are not available. The owner's \
                       session, by its local_ id."
    )]
    pub(crate) session: Option<String>,
    #[schemars(
        with = "Option<Slug>",
        description = "The owner's program, such as claude-code."
    )]
    pub(crate) program: Option<String>,
    #[schemars(
        with = "Option<Person>",
        description = "The owner's person, such as luke."
    )]
    pub(crate) person: Option<String>,
    #[schemars(
        regex(pattern = r"^/"),
        description = "One repo's tasks: / for the board's own tree, or a notes repo's mount path \
                       such as /repos/notes-personal; the caller's git remote chooses the repos \
                       when left out."
    )]
    pub(crate) repo: Option<String>,
    #[schemars(
        length(min = 1),
        description = "A git remote, or owner/name, to choose the repos by in place of the origin \
                       of the directory board was started in."
    )]
    pub(crate) remote: Option<String>,
}

/// An open task: where it is, why it waits, how many open tasks wait on
/// it, and its place in the tree.
///
/// `parent` is the parent's path in the board, or null when it has none,
/// names no task in its repo, or is where a loop was cut. `tree_path` joins
/// the slugs from its root to it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct SweptTask {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) due: Option<String>,
    pub(crate) defer: Option<String>,
    pub(crate) reviewed: Option<String>,
    pub(crate) contexts: Vec<String>,
    pub(crate) owner: Option<Actor>,
    pub(crate) blocked_by: Vec<String>,
    pub(crate) waiting: usize,
    pub(crate) parent: Option<String>,
    pub(crate) subtasks: Subtasks,
    pub(crate) tree_path: String,
}

/// At most 100 tasks of a section, by repo then title, and how many it
/// holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Section {
    pub(crate) tasks: Vec<SweptTask>,
    pub(crate) total: usize,
}

/// A repo that did not sync, and the code why; its tasks are as its clone
/// holds them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Unsynced {
    pub(crate) repo: String,
    pub(crate) code: String,
}

/// The sweep.
///
/// `available` holds the open tasks not deferred, not blocked and past
/// their `from_hour`. `due_soon` holds every open task due within seven
/// days, or overdue. `stale` holds the open tasks not available or deferred
/// that have no `reviewed`, or were reviewed over 14 days ago. With
/// `--owner`, every section keeps the tasks that owner or nobody holds, and
/// `held` lists the ones that owner holds that are not available; without
/// it, `held` is empty. `repos` are the repos read, `degraded` counts their
/// Markdown files under `tasks/` that are not a readable `tasks/task/v2`
/// task, or hold a date that cannot be read, and `unsynced` lists the repos
/// whose sync or pin failed. `cross_repo` lists the `after` and `parent`
/// entries that name another repo's task, which are not followed.
/// `origin_unread` says why the caller's git repository gave no origin,
/// when it did not, so only `/` and the personal repo were read.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "AvailableOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) available: Section,
    pub(crate) due_soon: Section,
    pub(crate) stale: Section,
    pub(crate) held: Section,
    pub(crate) repos: Vec<String>,
    pub(crate) degraded: usize,
    pub(crate) unsynced: Vec<Unsynced>,
    pub(crate) cross_repo: Vec<Crossing>,
    pub(crate) origin_unread: Option<String>,
}

/// Syncs the repos in scope, then sorts their open tasks into the sweep's
/// sections. The repos sync one after another, each within five seconds.
/// Only when a sync moves a clone past its pin does the call move that pin
/// and read the tasks again.
pub(crate) fn available(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "available", input)?;
    let (name, remote) = (given.repo.as_deref(), given.remote.as_deref());
    let (remote, unread) = chosen_by(host, name, remote);
    let (found, mut rows) = listing(host)?;
    let roots = scope(PLUGIN, &found, name, remote.as_deref(), unread.as_deref())?;
    let clones = Through(host);
    let writer = writer(host, &clones);
    let mut unsynced = Vec::new();
    let mut moved = false;
    for listed in found.iter().filter(|listed| roots.contains(&listed.root)) {
        let Some(clone) = &listed.clone else { continue };
        let mut failed = None;
        match sync(host, clone) {
            Err(code) => failed = Some(code),
            Ok(head) if listed.source.as_deref() != Some(head.as_str()) => {
                let repo = Repo {
                    root: listed.root.clone(),
                    clone: Some(clone.clone()),
                };
                match repo.pin(writer) {
                    Ok(_) => moved = true,
                    Err(error) => {
                        let code = error.fields.get("code").and_then(Value::as_str);
                        failed = Some(code.map_or(error.code.clone(), str::to_owned));
                    }
                }
            }
            Ok(_) => {}
        }
        if let Some(code) = failed {
            unsynced.push(Unsynced {
                repo: listed.root.clone(),
                code,
            });
        }
    }
    if moved {
        rows = listing(host)?.1;
    }
    let settings = host.settings();
    let (tasks, degraded) = read(&rows, &roots, settings.utc_offset);
    let links = resolve(host, &tasks)?;
    let (nodes, tree) = grown(&tasks, &links);
    let mut open: Vec<Task> = tasks
        .into_iter()
        .filter(|task| task.status == "open")
        .collect();
    let blocked: BTreeMap<String, Vec<String>> = (open.iter())
        .map(|task| (task.path.clone(), waited(&links, &tree, &nodes, &task.path)))
        .collect();
    if let Some(context) = &given.context {
        open.retain(|task| task.contexts.contains(context));
    }
    let clock = Clock {
        now: seconds(settings.now),
        offset: settings.utc_offset,
        start: day_start(settings.day_start.as_deref()),
        day: today(
            settings.now,
            settings.utc_offset,
            day_start(settings.day_start.as_deref()),
        ),
    };
    let places = Places {
        tree: &tree,
        nodes: &nodes,
    };
    let owner = actor(given.session, given.program, given.person);
    let [available, due_soon, stale, held] = sweep(&open, &blocked, places, &clock, owner.as_ref());
    Ok(json!(Output {
        available,
        due_soon,
        stale,
        held,
        repos: roots,
        degraded,
        unsynced,
        cross_repo: links.crossing,
        origin_unread: unread,
    }))
}

/// Syncs the clone without pushing: HEAD after it, or the code of a sync
/// that failed or ran past its deadline.
fn sync(host: &dyn Host, clone: &str) -> Result<String, String> {
    let board = (host.board(&Elsewhere::Directory(PathBuf::from(clone))))
        .map_err(|refusal| refusal.code.as_str().to_owned())?;
    match board.sync(false, SYNC_DEADLINE) {
        Ok(synced) => Ok(synced.new.to_string()),
        Err(error) if error.code() == Code::SyncTimeout => Err(SYNC_TIMEOUT.to_owned()),
        Err(error) => Err(error.code().as_str().to_owned()),
    }
}

/// `moment` in seconds since 1970-01-01T00:00Z.
fn seconds(moment: SystemTime) -> i64 {
    match moment.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// The clock a sweep reads: the moment, in seconds since 1970-01-01T00:00Z,
/// the seconds east of UTC, when the person's day starts after midnight,
/// and the person's today.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    pub(crate) now: i64,
    pub(crate) offset: i32,
    pub(crate) start: Duration,
    pub(crate) day: Date,
}

/// The tree the tasks' places are read from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Places<'a> {
    pub(crate) tree: &'a Forest,
    pub(crate) nodes: &'a Nodes,
}

/// The sections of the sweep of open `tasks`: available, due soon, stale
/// and held. It does no I/O. With an `owner`, every section keeps the
/// tasks that owner or nobody holds, and `held` lists the ones that owner
/// holds that are not available. `waiting` still counts every task.
pub(crate) fn sweep(
    tasks: &[Task],
    blocked: &BTreeMap<String, Vec<String>>,
    places: Places<'_>,
    clock: &Clock,
    owner: Option<&Actor>,
) -> [Section; 4] {
    let mut waiting: BTreeMap<&str, usize> = BTreeMap::new();
    for target in blocked.values().flatten() {
        *waiting.entry(target).or_default() += 1;
    }
    let mut shown: [Vec<SweptTask>; 4] = Default::default();
    let mut sorted: Vec<&Task> = tasks.iter().collect();
    sorted.sort_by(|one, other| {
        (&one.repo, &one.title, &one.path).cmp(&(&other.repo, &other.title, &other.path))
    });
    let day = clock.day;
    for task in sorted {
        if owner.is_some() && task.owner.is_some() && task.owner.as_ref() != owner {
            continue;
        }
        let waits = blocked.get(&task.path).cloned().unwrap_or_default();
        let parked = deferred(task, clock);
        let free = !parked && waits.is_empty() && opened(task.from_hour, clock);
        let old = !parked
            && task
                .reviewed
                .is_none_or(|reviewed| reviewed < day.plus(-STALE));
        let item = SweptTask {
            repo: task.repo.clone(),
            path: task.path.clone(),
            title: task.title.clone(),
            due: task.due.map(|due| due.to_string()),
            defer: task.defer.as_ref().map(|(text, _)| text.clone()),
            reviewed: task.reviewed.map(|reviewed| reviewed.to_string()),
            contexts: task.contexts.clone(),
            owner: task.owner.clone(),
            blocked_by: waits,
            waiting: waiting.get(task.path.as_str()).copied().unwrap_or(0),
            parent: places.tree.parents.get(&task.path).cloned(),
            subtasks: subtasks(places.tree, places.nodes, &task.path),
            tree_path: tree_path(places.tree, &task.path),
        };
        let due = task.due.is_some_and(|due| due <= day.plus(DUE_SOON));
        let held = !free && owner.is_some() && task.owner.as_ref() == owner;
        for (keep, section) in [free, due, !free && old, held].into_iter().zip(&mut shown) {
            if keep {
                section.push(item.clone());
            }
        }
    }
    shown.map(|items| Section {
        total: items.len(),
        tasks: items.into_iter().take(SHOWN).collect(),
    })
}

/// Whether the task is deferred past now. A horizon counts from
/// `reviewed`; without one, it defers nothing.
fn deferred(task: &Task, clock: &Clock) -> bool {
    match task.defer.as_ref().map(|(_, defer)| *defer) {
        None => false,
        Some(Defer::Horizon(days)) => task
            .reviewed
            .is_some_and(|reviewed| clock.day < reviewed.plus(days)),
        Some(Defer::Until(Until::Moment(moment))) => clock.now < moment,
        Some(Defer::Until(Until::Date(date))) => clock.day < date,
    }
}

/// Whether the person's day has reached `hour`: an hour before the day
/// starts falls on the next date.
fn opened(hour: Option<u8>, clock: &Clock) -> bool {
    let Some(hour) = hour else { return true };
    let midnight = clock.day.days() * 86_400 - i64::from(clock.offset);
    let start = i64::try_from(clock.start.as_secs()).unwrap_or(i64::MAX);
    let mut at = midnight + i64::from(hour) * 3600;
    if at < midnight + start {
        at += 86_400;
    }
    clock.now >= at
}

#[cfg(test)]
mod tests;

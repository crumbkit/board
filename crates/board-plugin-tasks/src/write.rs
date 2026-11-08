//! The write calls: `add`, `done`, `defer`, `drop`, `reviewed`
//! and `move` (plugins/tasks/README.md, Writing tasks).
//!
//! Each call syncs a mounted repo before it writes and pushes after, as
//! notes `add` does, and each but `move` sets `reviewed` to the person's
//! today.

use std::path::Path;

use board_model::Source;
use board_ops::{Host, PluginError};
use board_plugin::shape::{Actor, Record as _};
use board_repos::{
    ATTEMPTS, Date, EDITED, Listed, MOVED, Repo, Through, Writer, covering, create, day_start,
    landed, landed_with, listed, push, refused, root, slug, synced, today, trimmed,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::dates::horizon;
use crate::lines::with_fields;
use crate::listing::{grown, held_by, listing, read, resolve};
use crate::records::TaskRecord;
use crate::repeat::completed;
use crate::task::model;
use crate::tree::{Forest, descendants};
use crate::{PLUGIN, caller_origin, refusal, writer};

mod inputs;

pub(crate) use inputs::{AddInput, DeferInput, DoneInput, MoveInput, TaskPath};

/// The refusal of a write that found the task busy: edited in its clone
/// and not committed, or changing.
const BUSY: &str = "tasks::task::busy";
/// How many names a new task tries, `<slug>.md`, then `<slug>-2.md` and on.
const NAMES: usize = 10;

/// The task as a write call left it.
///
/// `commit` holds the write, null when the board's own tree left it
/// uncommitted. `unsynced` is the code of a sync before the write that left
/// the clone where it was, and `unpushed` the code of a push after it that
/// failed, or `tasks::repo::not_pushed` when `done --push false` skipped
/// it; both are `tasks::repo::not_synced` for the board's own tree, which
/// is never synced.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Written {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) status: String,
    pub(crate) defer: Option<String>,
    pub(crate) owner: Option<Actor>,
    pub(crate) reviewed: String,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
    pub(crate) unpushed: Option<String>,
}

/// The task as `move` left it: `parent` as given, or null once cleared.
/// The other fields are as in `Written`.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Moved {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) parent: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
    pub(crate) unpushed: Option<String>,
}

/// The fields a call sets, each to a value or, as none, removed.
pub(crate) type Values = Vec<(&'static str, Option<Value>)>;

/// The person's today, by the call's time zone and `day_start`.
fn day(host: &dyn Host) -> Date {
    let settings = host.settings();
    let start = day_start(settings.day_start.as_deref());
    today(settings.now, settings.utc_offset, start)
}

/// `value` as Python's `json.dumps` writes it, keeping non-ASCII text,
/// which is YAML too.
fn dumped(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(dumped).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}

/// Writes a new open task to `tasks/<slug>.md` in the repo the caller's
/// remote routes to.
pub(crate) fn add(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: AddInput = PluginError::read_input(PLUGIN, "add", input)?;
    let remote = match &given.remote {
        Some(remote) => Some(remote.clone()),
        None => origin(host, given.repo.as_deref())?,
    };
    let found = listed(host, PLUGIN)?;
    let repo = routed(&found, given.repo.as_deref(), remote.as_deref())?;
    let day = day(host);
    let mut fields: Vec<(&str, Value)> = vec![
        ("$type", json!(TaskRecord::TYPE)),
        ("title", json!(given.title)),
        ("status", json!("open")),
    ];
    let added = [
        ("kind", given.kind.as_ref().map(|kind| json!(kind))),
        ("brief", given.brief.as_ref().map(|brief| json!(brief))),
        ("parent", given.parent.as_ref().map(|parent| json!(parent))),
        ("due", given.due.as_ref().map(|due| json!(due))),
        (
            "defer",
            given.defer.as_ref().map(|defer| json!(until(defer, day))),
        ),
        ("after", given.after.as_ref().map(|after| json!(after))),
        ("repeat", given.repeat.as_ref().map(|repeat| json!(repeat))),
        (
            "ephemeral",
            given.ephemeral.map(|ephemeral| json!(ephemeral)),
        ),
        ("from_hour", given.from_hour.map(|hour| json!(hour))),
        (
            "contexts",
            given.contexts.as_ref().map(|contexts| json!(contexts)),
        ),
    ];
    for (key, value) in added {
        let Some(mut value) = value else { continue };
        if let ("brief" | "parent", Some(path)) = (key, value.as_str()) {
            value = json!(in_repo(&found, &repo, path, &[(key, json!(path))], "add")?);
        }
        fields.push((key, value));
    }
    fields.push(("reviewed", json!(day.to_string())));
    let mut text = "---\n".to_owned();
    for (key, value) in &fields {
        text.extend([*key, ": ", dumped(value).as_str(), "\n"]);
    }
    text += "---\n";
    if let Some(body) = &given.text {
        text.extend(["\n", trimmed(body), "\n"]);
    }
    let clones = Through(host);
    let writer = writer(host, &clones);
    let synced = push(writer, &repo);
    if let Some(parent) = &given.parent {
        repo.pin(writer)?;
        parented(host, &repo, parent, &[("parent", json!(parent))], "add")?;
    }
    let stem = format!("tasks/{}", slug(&given.title));
    let (path, commit) = create(writer, &repo, &stem, &text, NAMES, BUSY)?;
    let record: Map<String, Value> = (fields.into_iter())
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    let (commit, unpushed) = landed(writer, &repo, commit)?;
    Ok(json!(written(
        &repo,
        &path,
        &record,
        (commit, synced, unpushed)
    )))
}

/// The caller's origin to route a new task by, unless `repo` names its
/// repo; refused when git gave none. A caller inside a git repository whose
/// origin git did not give may be in a work repository, so the task is not
/// written to the personal repo.
fn origin(host: &dyn Host, repo: Option<&str>) -> Result<Option<String>, PluginError> {
    if repo.is_some() {
        return Ok(None);
    }
    match caller_origin(host) {
        (_, Some(unread)) => {
            let why = format!(
                "{unread}, so the repo the task belongs to is not known; name it with --remote or \
                 --repo"
            );
            Err(refusal(
                "tasks::repo::origin_unread",
                why,
                [("origin_unread", json!(unread))],
            ))
        }
        (remote, None) => Ok(remote),
    }
}

/// Each value of `values` as a JSON string.
fn json_values(values: Vec<(&'static str, Option<String>)>) -> Values {
    (values.into_iter())
        .map(|(key, value)| (key, value.map(Value::from)))
        .collect()
}

/// Marks the task done, or defers a repeating one to its next occurrence.
pub(crate) fn done(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: DoneInput = PluginError::read_input(PLUGIN, "done", input)?;
    stamped(
        host,
        &given.path,
        &|record, day| Ok(json_values(completed(record, day))),
        Landing {
            finished: false,
            push: given.push != Some(false),
        },
    )
}

/// Defers the task until a date, writing a horizon as the date it ends on.
pub(crate) fn defer(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: DeferInput = PluginError::read_input(PLUGIN, "defer", input)?;
    let change =
        |_: &Map<String, Value>, day| Ok(vec![("defer", Some(until(&given.until, day).into()))]);
    stamped(host, &given.path, &change, Landing::open())
}

/// Marks the task dropped.
pub(crate) fn drop(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: TaskPath = PluginError::read_input(PLUGIN, "drop", input)?;
    let change = |_: &Map<String, Value>, _| Ok(vec![("status", Some("dropped".into()))]);
    stamped(host, &given.path, &change, Landing::open())
}

/// Marks the task reviewed today, whatever its status.
pub(crate) fn reviewed(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: TaskPath = PluginError::read_input(PLUGIN, "reviewed", input)?;
    stamped(
        host,
        &given.path,
        &|_, _| Ok(Vec::new()),
        Landing {
            finished: true,
            push: true,
        },
    )
}

/// Which tasks a write call takes, and whether it pushes: an open task
/// unless `finished`, and the task's repo is pushed before and after the
/// write when `push`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Landing {
    pub(crate) finished: bool,
    pub(crate) push: bool,
}

impl Landing {
    /// An open task, pushed.
    pub(crate) fn open() -> Self {
        Self {
            finished: false,
            push: true,
        }
    }
}

/// Sets the fields `change` names and `reviewed` on the task at `logical`,
/// as `landing` says, and gives the written task.
pub(crate) fn stamped(
    host: &dyn Host,
    logical: &str,
    change: &Change<'_>,
    landing: Landing,
) -> Result<Value, PluginError> {
    let found = listed(host, PLUGIN)?;
    let repo = held(&found, logical);
    let stamp = |record: &Map<String, Value>, day: Date| {
        let mut values = change(record, day)?;
        values.push(("reviewed", Some(day.to_string().into())));
        Ok(values)
    };
    let clones = Through(host);
    let writer = writer(host, &clones);
    let edited = edit(writer, &repo, logical, &stamp, landing)?;
    let (commit, unpushed) = landed_with(writer, &repo, edited.commit, landing.push)?;
    Ok(json!(written(
        &repo,
        &edited.path,
        &edited.record,
        (commit, edited.synced, unpushed)
    )))
}

/// Sets the task's parent to another task in its repo, or clears it with
/// `none`, and changes nothing else. The new parent is refused when it is
/// in another repo, holds no task, or is the task or one of its
/// descendants, which would make a loop. The tree is read after the sync,
/// on every attempt.
pub(crate) fn move_task(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: MoveInput = PluginError::read_input(PLUGIN, "move", input)?;
    let (logical, parent) = (given.path.as_str(), given.parent.as_str());
    let found = listed(host, PLUGIN)?;
    let repo = held(&found, logical);
    let fields = [("path", json!(logical)), ("parent", json!(parent))];
    let cleared = parent == "none";
    let written_as = if cleared {
        None
    } else {
        Some(in_repo(&found, &repo, parent, &fields, "move")?)
    };
    let change = |_: &Map<String, Value>, _: Date| -> Result<Values, PluginError> {
        if cleared {
            return Ok(vec![("parent", None)]);
        }
        let tree = parented(host, &repo, parent, &fields, "move")?;
        if parent == logical
            || descendants(&tree, logical)
                .iter()
                .any(|each| each == parent)
        {
            let why = format!(
                "{parent} is {logical} or under it, so the move would make a loop; give a task \
                 outside its tree"
            );
            return Err(refusal("tasks::move::cycle", why, fields.clone()));
        }
        Ok(vec![("parent", written_as.clone().map(Value::from))])
    };
    let clones = Through(host);
    let writer = writer(host, &clones);
    let landing = Landing {
        finished: true,
        push: true,
    };
    let edited = edit(writer, &repo, logical, &change, landing)?;
    let (commit, unpushed) = landed(writer, &repo, edited.commit)?;
    Ok(json!(Moved {
        repo: repo.root.clone(),
        path: repo.logical(&edited.path),
        parent: (!cleared).then(|| parent.to_owned()),
        commit,
        unsynced: edited.synced,
        unpushed,
    }))
}

/// The board path `path` as a path in `repo`; refused with
/// `tasks::<call>::cross_repo` in another repo.
fn in_repo(
    found: &[Listed],
    repo: &Repo,
    path: &str,
    fields: &[(&str, Value)],
    call: &str,
) -> Result<String, PluginError> {
    if held_by(found, path) != repo.root {
        let why = format!(
            "{path} is in another repo than {}, and a task's references stay in its repo; name \
             one in it",
            repo.root
        );
        return Err(refusal(
            &format!("tasks::{call}::cross_repo"),
            why,
            fields.iter().cloned(),
        ));
    }
    let prefix = repo.root.trim_end_matches('/');
    Ok(path.strip_prefix(prefix).unwrap_or(path).to_owned())
}

/// The tree of `repo`, refused with `tasks::<call>::parent_not_found` when
/// `parent` holds none of its tasks.
fn parented(
    host: &dyn Host,
    repo: &Repo,
    parent: &str,
    fields: &[(&str, Value)],
    call: &str,
) -> Result<Forest, PluginError> {
    let rows = listing(host)?.1;
    let (tasks, _) = read(
        &rows,
        std::slice::from_ref(&repo.root),
        host.settings().utc_offset,
    );
    if !tasks.iter().any(|task| task.path == parent) {
        let why = format!(
            "no task is at {parent}; give the path of a task, as `board tasks tree --root \
             true` lists them"
        );
        return Err(refusal(
            &format!("tasks::{call}::parent_not_found"),
            why,
            fields.iter().cloned(),
        ));
    }
    let links = resolve(host, &tasks)?;
    Ok(grown(&tasks, &links).1)
}

/// What a call changes, from the task's record and the person's today.
type Change<'a> = dyn Fn(&Map<String, Value>, Date) -> Result<Values, PluginError> + 'a;

/// A task as an edit left it: its path in its repo, its record with the
/// change made, the commit that holds it, and the code of the sync before.
struct Edited {
    path: String,
    record: Map<String, Value>,
    commit: Option<String>,
    synced: Option<String>,
}

/// Sets the fields `change` names on the task at `logical` in `repo`, as
/// `landing` says. The sync before the write pushes only when
/// `landing.push`.
///
/// A write refused because the task or the clone moved since it was read is
/// tried again on a new read. A new read that finds the text the last
/// write sent, which Board wrote before it refused, is that write.
fn edit(
    writer: Writer<'_>,
    repo: &Repo,
    logical: &str,
    change: &Change<'_>,
    landing: Landing,
) -> Result<Edited, PluginError> {
    let host = writer.host;
    let prefix = format!("{}/", repo.root.trim_end_matches('/'));
    let path = logical.strip_prefix(&prefix).unwrap_or(logical).to_owned();
    let synced = synced(writer, repo, landing.push);
    let day = day(host);
    let (mut moved, mut sent) = (None, None::<String>);
    for _ in 0..ATTEMPTS {
        repo.pin(writer)?;
        let task = repo.read(host, PLUGIN, &path)?;
        if let (Some(sent), Some(Value::Object(record))) = (&sent, &task.record)
            && task.text.as_ref() == Some(sent)
        {
            return Ok(Edited {
                path,
                record: record.clone(),
                commit: None,
                synced,
            });
        }
        let record = checked(host, &task, repo, logical, landing.finished)?;
        let values = change(&record, day)?;
        let text = with_fields(task.text.as_deref().unwrap_or(""), &values, logical)?;
        let error = match repo.write(host, &path, &text, task.blob.as_deref()) {
            Ok(commit) => {
                let mut record = record;
                for (key, value) in values {
                    record.insert(key.to_owned(), value.map_or(Value::Null, Value::from));
                }
                return Ok(Edited {
                    path,
                    record,
                    commit,
                    synced,
                });
            }
            Err(error) => error,
        };
        sent = Some(text);
        let code = refused(&error);
        if code == EDITED {
            let why = format!(
                "{logical} changed in {} after it was read; call again, and if this persists, \
                 commit or discard the edit not committed there",
                repo.clone.as_deref().unwrap_or("the board's own tree")
            );
            return Err(refusal(
                BUSY,
                why,
                [("path", json!(logical)), ("code", json!(code.as_str()))],
            ));
        }
        if !MOVED.contains(&code) {
            return Err(PluginError::board_refused(PLUGIN, "put", &error.refusal()));
        }
        moved = Some(code);
    }
    let why = format!(
        "{logical} kept changing while it was written, {ATTEMPTS} times; read it with `board get \
         {logical}`, and call again if it lacks the change"
    );
    Err(refusal(
        BUSY,
        why,
        [
            ("path", json!(logical)),
            ("code", json!(moved.map(board_model::Code::as_str))),
        ],
    ))
}

/// The task's record, refused unless it is a valid v2 task under `tasks/`
/// in a notes repo, open unless `finished`.
///
/// The file is in the notes repo `held` chose only when Board read it from
/// that repo's clone, or from the board's own tree for `/`. A task in a
/// mount that is not a notes repo fails that.
fn checked(
    host: &dyn Host,
    task: &board_repos::File,
    repo: &Repo,
    logical: &str,
    finished: bool,
) -> Result<Map<String, Value>, PluginError> {
    let at = [("path", json!(logical))];
    if task.blob.is_none() {
        let why = format!("no file is at {logical}; list the tasks with `board tasks available`");
        return Err(refusal("tasks::task::not_found", why, at.clone()));
    }
    let board = task.location.as_ref().map(|location| &location.board);
    let held = match &repo.clone {
        None => board.is_some_and(|board| board.source == Source::WorkingTree),
        Some(clone) => board.is_some_and(|board| board.repository == Path::new(clone)),
    };
    if !held {
        let why = format!(
            "{logical} is not in a notes repo, the board's own tree or a mount whose tree has \
             daily/, so no call writes it; give a path under one, as `board tasks available` \
             lists them"
        );
        return Err(refusal("tasks::repo::not_found", why, at.clone()));
    }
    let hint = format!("run `board check {logical}` to see what it holds");
    let within = logical
        .strip_prefix(repo.root.trim_end_matches('/'))
        .unwrap_or(logical);
    if task.text.is_none() || !within.contains("/tasks/") {
        let why = format!(
            "{logical} is not a {} task under a tasks/ directory; {hint}",
            TaskRecord::TYPE
        );
        return Err(refusal("tasks::task::not_task", why, at.clone()));
    }
    let record = match &task.record {
        Some(Value::Object(record)) => record.clone(),
        _ => Map::new(),
    };
    if let Err(errors) = model(&Value::Object(record.clone()), host.settings().utc_offset) {
        let why = format!(
            "{logical} is not a valid {} task ({errors} errors); {hint}",
            TaskRecord::TYPE
        );
        return Err(refusal("tasks::task::not_task", why, at.clone()));
    }
    let status = record.get("status").cloned().unwrap_or(Value::Null);
    if !finished && status != "open" {
        let shown = status.as_str().unwrap_or("not open");
        let why = format!(
            "{logical} is {shown}, so it is left as it is; set `status: open` in it to reopen it \
             first"
        );
        return Err(refusal(
            "tasks::task::finished",
            why,
            [("path", json!(logical)), ("status", status)],
        ));
    }
    Ok(record)
}

/// The repo a new task goes to: the one named; else the one covering the
/// remote; else the personal one; else `/`. Two repos covering the remote,
/// or two personal ones, are refused, since a task must not cross a silo.
fn routed(found: &[Listed], name: Option<&str>, remote: Option<&str>) -> Result<Repo, PluginError> {
    let clone_of = |at: &str| {
        (at != "/")
            .then(|| found.iter().find(|listed| listed.root == at))
            .flatten()
            .and_then(|listed| listed.clone.clone())
    };
    if let Some(name) = name {
        let at = root(name);
        let mut repos: Vec<String> = found.iter().map(|listed| listed.root.clone()).collect();
        repos.push("/".to_owned());
        repos.sort();
        repos.dedup();
        if !repos.contains(&at) {
            let why = format!(
                "no repo is at {name} (repos: {}); name one listed with --repo",
                repos.join(", ")
            );
            return Err(refusal(
                "tasks::repo::not_found",
                why,
                [("repos", json!(repos))],
            ));
        }
        return Ok(Repo {
            clone: clone_of(&at),
            root: at,
        });
    }
    let (covered, personal) = covering(PLUGIN, found, remote)?;
    let candidates = if covered.is_empty() {
        personal
    } else {
        covered
    };
    if candidates.len() > 1 {
        let why = format!(
            "{} could each hold the task, so none was written; name one with --repo",
            candidates.join(", ")
        );
        return Err(refusal(
            "tasks::repo::ambiguous",
            why,
            [("candidates", json!(candidates))],
        ));
    }
    let at = candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| "/".to_owned());
    Ok(Repo {
        clone: clone_of(&at),
        root: at,
    })
}

/// The repo of `found` that holds the board path `logical`, with its clone.
fn held(found: &[Listed], logical: &str) -> Repo {
    let at = held_by(found, logical);
    let clone = (found.iter())
        .find(|listed| listed.root == at)
        .and_then(|listed| listed.clone.clone());
    Repo { root: at, clone }
}

/// The date a `defer` ends: a date as it is, or a horizon counted from
/// `day`.
fn until(value: &str, day: Date) -> String {
    horizon(value).map_or_else(|| value.to_owned(), |days| day.plus(days).to_string())
}

/// The call's output, once the write is landed: its commit, the code of
/// the sync before it and the code of the push after it.
fn written(
    repo: &Repo,
    path: &str,
    record: &Map<String, Value>,
    (commit, synced, unpushed): (Option<String>, Option<String>, Option<String>),
) -> Written {
    let text = |key: &str| {
        (record.get(key))
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            })
    };
    Written {
        repo: repo.root.clone(),
        path: repo.logical(path),
        status: text("status").unwrap_or_default(),
        defer: text("defer"),
        owner: (record.get("owner")).and_then(|owner| Actor::deserialize(owner).ok()),
        reviewed: text("reviewed").unwrap_or_default(),
        commit,
        unsynced: synced,
        unpushed,
    }
}

#[cfg(test)]
mod tests;

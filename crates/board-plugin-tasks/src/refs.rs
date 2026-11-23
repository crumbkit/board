//! `ref`: adds a record or file to a v3 task's `refs`, or removes one
//! (plugins/tasks/README.md, Refs). A ref names a file in the task's own
//! repository; a crossing between boards is an exchange message, and the
//! task names its own copy of it (docs/guide/exchange.md).

use board_model::{Code, LogicalPath};
use board_ops::{Host, PluginError};
use board_plugin::shape::Record as _;
use board_repos::{Date, Repo, Through, landed, listed};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::records::{MOST_REFS, TaskRecord};
use crate::write::{Landing, RefInput, Values, edit, held};
use crate::{PLUGIN, hinted, refusal, writer};

/// The task as `ref` left it: `refs` holds every entry, as paths in the
/// board, and `unchanged` says the call found nothing to change and wrote
/// nothing. The other fields are as in `Written`.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Reffed {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) refs: Vec<String>,
    pub(crate) unchanged: bool,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
    pub(crate) unpushed: Option<String>,
    pub(crate) conflicts: Vec<String>,
}

/// Adds `--add` to the end of the task's `refs`, once, and removes
/// `--remove`, keeping the order of the rest and every other byte. It reads
/// the task after the sync and writes nothing when the refs stay as they
/// were.
pub(crate) fn ref_task(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: RefInput = PluginError::read_input(PLUGIN, "ref", input)?;
    let logical = given.path.as_str();
    let found = listed(host, PLUGIN)?;
    let repo = held(&found, logical);
    let change = |record: &Map<String, Value>, day: Date| -> Result<Values, PluginError> {
        if record.get("$type") != Some(&json!(TaskRecord::TYPE)) {
            return Err(not_v3(logical, "ref"));
        }
        let before: Vec<String> = (record.get("refs").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect();
        let mut refs = before.clone();
        if let Some(add) = &given.add {
            let written = local_ref(host, &repo, add)?;
            if !refs.contains(&written) {
                refs.push(written);
            }
        }
        if let Some(remove) = &given.remove {
            let written = within(&repo, remove);
            refs.retain(|held| *held != written);
        }
        if refs.len() > MOST_REFS {
            return Err(too_many(logical, refs.len()));
        }
        if refs == before {
            return Ok(Vec::new());
        }
        Ok(vec![
            ("refs", (!refs.is_empty()).then(|| json!(refs))),
            ("reviewed", Some(day.to_string().into())),
        ])
    };
    let clones = Through::new(host);
    let writer = writer(host, &clones);
    let landing = Landing {
        finished: true,
        push: true,
    };
    let edited = edit(writer, &repo, logical, &change, landing)?;
    let (commit, unpushed) = landed(writer, &repo, edited.commit)?;
    let refs = (edited.record.get("refs").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|path| in_board(&repo, path))
        .collect();
    Ok(json!(Reffed {
        repo: repo.root.clone(),
        path: repo.logical(&edited.path),
        refs,
        unchanged: !edited.wrote,
        commit,
        unsynced: edited.synced,
        unpushed,
        conflicts: writer.conflicts_at(&repo, &[&edited.path]),
    }))
}

/// Each of `paths`, board paths, as the refs a new task in `repo` holds:
/// once each, in order, and no more than [`MOST_REFS`].
pub(crate) fn local_refs(
    host: &dyn Host,
    repo: &Repo,
    paths: &[String],
) -> Result<Vec<String>, PluginError> {
    let mut refs: Vec<String> = Vec::new();
    for path in paths {
        let written = local_ref(host, repo, path)?;
        if !refs.contains(&written) {
            refs.push(written);
        }
    }
    if refs.len() > MOST_REFS {
        return Err(too_many("the new task", refs.len()));
    }
    Ok(refs)
}

/// The board path `path` as a ref a task in `repo` holds: its path in the
/// repo. Refused with `tasks::ref::missing` when no file is there, and with
/// `tasks::ref::through_mount` when it is in another board than the
/// repo's own, which only a mount reaches.
fn local_ref(host: &dyn Host, repo: &Repo, path: &str) -> Result<String, PluginError> {
    let fields = [("ref", json!(path))];
    let missing = || {
        let why = format!(
            "no file is at {path}; a ref names a record or file in the task's own repository, so \
             write it first"
        );
        refusal("tasks::ref::missing", why, fields.clone())
    };
    let target: LogicalPath = path.parse().map_err(|_| missing())?;
    let refused = |error: &board_ops::ReadError| {
        PluginError::board_refused(PLUGIN, "get", &error.refusal("get"))
    };
    let answers = host.get_each(&[target]).map_err(|error| refused(&error))?;
    let location = match answers.into_iter().next() {
        Some(Ok(report)) => report.location,
        Some(Err(error))
            if [Code::PathNotFound, Code::RecordNotARecord].contains(&error.code()) =>
        {
            return Err(missing());
        }
        Some(Err(error)) => return Err(refused(&error)),
        None => return Err(missing()),
    };
    // The repo's own mount, the one a path in a notes repo crosses.
    let own: Vec<String> = (repo.root != "/")
        .then(|| repo.root.clone())
        .into_iter()
        .collect();
    let crossed: Vec<String> = location.mounts().iter().map(ToString::to_string).collect();
    if crossed != own {
        let mount = (crossed.iter())
            .find(|mount| !own.contains(mount))
            .cloned()
            .unwrap_or_else(|| "/".to_owned());
        let why = format!(
            "{path} is in the board at {mount}, outside the task's own repository {}, and a \
             record never names a path in another board",
            repo.root
        );
        return Err(hinted(
            "tasks::ref::through_mount",
            why,
            [("ref", json!(path)), ("mount", json!(mount))],
            "a crossing between boards is an exchange message (docs/guide/exchange.md): send \
             the request, then name your copy of it under /exchange/sent/",
        ));
    }
    Ok(within(repo, path))
}

/// The board path `path` as a path in `repo`.
fn within(repo: &Repo, path: &str) -> String {
    let prefix = repo.root.trim_end_matches('/');
    match path.strip_prefix(prefix) {
        Some(rest) if rest.starts_with('/') => rest.to_owned(),
        _ => path.to_owned(),
    }
}

/// The path `path` in `repo` as a path in the board.
fn in_board(repo: &Repo, path: &str) -> String {
    if repo.root == "/" {
        path.to_owned()
    } else {
        repo.logical(path.trim_start_matches('/'))
    }
}

/// The refusal of refs on a `tasks/task/v2` task, `what`.
pub(crate) fn not_v3(what: &str, call: &str) -> PluginError {
    let why = format!(
        "{what} is a tasks/task/v2 task, which holds no refs; only a tasks/task/v3 task does"
    );
    hinted(
        "tasks::ref::not_v3",
        why,
        [("path", json!(what)), ("call", json!(call))],
        "run `board tasks migrate` to list the v2 tasks, then `board tasks migrate --yes` to \
         rewrite them as v3",
    )
}

/// The refusal of more than [`MOST_REFS`] refs on `what`.
fn too_many(what: &str, count: usize) -> PluginError {
    let why = format!(
        "{what} would hold {count} refs, and a task holds at most {MOST_REFS}; nothing was written"
    );
    hinted(
        "tasks::ref::too_many",
        why,
        [("path", json!(what)), ("refs", json!(count))],
        "split the work into child tasks with `board tasks add --parent <task>`, each naming its \
         own refs",
    )
}

//! `migrate`: lists each `tasks/task/v2` task that is not a project and the
//! `tasks/task/v3` record it becomes, and with `--yes` rewrites them, one
//! bulk commit per repo (plugins/tasks/README.md, Migrating to v3).
//!
//! The rewrite sets `$type` and removes `kind`, which a v2 task that is not
//! a project holds only as `task`, and keeps every other byte. A project
//! task stays v2 until projects move to their own records, so while one
//! remains `--yes` refuses and writes nothing. `plans/question/v1` and
//! `artifacts/artifact/v1` accept only a v2 task, so the listing names each
//! question and artifact that names a task it rewrites.

use std::collections::BTreeMap;

use board_model::LogicalPath;
use board_ops::{Bulk, Host, Limit, PluginError};
use board_plugin::shape::Record as _;
use board_repos::{Repo, Through, json_body, landed, listed, push, root, shown};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::lines::with_fields;
use crate::listing::listing;
use crate::records::{TaskRecord, V2_TYPE};
use crate::write::MigrateInput;
use crate::{PLUGIN, hinted, writer};

/// The most tasks a repo's listing names; `total` counts them all.
const SHOWN: usize = 100;
/// The most project tasks a refusal or a listing names.
const SHOWN_PROJECTS: usize = 20;
/// The most questions and artifacts the listing names.
const SHOWN_NAMING: usize = 100;
/// The reason the bulk commit's trailer gives.
const BULK: &str = "tasks migrate to tasks/task/v3";

/// A v2 task and the v3 record it becomes.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Migrated {
    pub(crate) path: String,
    pub(crate) record: Value,
}

/// A question or artifact, `path`, that names the task `task`, which
/// `board check` reports as `board::ref::wrong_type` once the task is v3.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Naming {
    pub(crate) path: String,
    pub(crate) task: String,
}

/// A repo whose rewrite was refused: the call's code and message.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Refused {
    pub(crate) code: String,
    pub(crate) message: String,
}

/// One repo's v2 tasks: at most 100, by path, and how many there are. With
/// `--yes`, `commit` holds the rewrite, null when nothing was written or the
/// board's own tree left it uncommitted; `unsynced` and `unpushed` are the
/// codes of the sync before and the push after, as a write call's are.
/// `refused` holds the code and message of a rewrite refused in this repo,
/// which leaves the other repos' rewrites as they are.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct RepoMigration {
    pub(crate) repo: String,
    pub(crate) tasks: Vec<Migrated>,
    pub(crate) total: usize,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
    pub(crate) unpushed: Option<String>,
    pub(crate) refused: Option<Refused>,
}

/// What `migrate` found, and wrote with `--yes`.
///
/// `repos` lists each repo read, `/` first. `projects` names at most 20
/// project tasks, which stay v2, and `projects_total` counts them.
/// `naming` lists at most 100 questions and artifacts that name a task the
/// run rewrites, which `board check` reports as `board::ref::wrong_type`
/// until question and artifact types accept a v3 task; `naming_total`
/// counts them. `wrote` says whether `--yes` rewrote any task, and
/// `unchanged` that no task was left to rewrite.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "MigrateOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) repos: Vec<RepoMigration>,
    pub(crate) projects: Vec<String>,
    pub(crate) projects_total: usize,
    pub(crate) naming: Vec<Naming>,
    pub(crate) naming_total: usize,
    pub(crate) wrote: bool,
    pub(crate) unchanged: bool,
}

/// One repo's tasks to rewrite, by their path in the board, with each
/// one's v2 record.
type Found = BTreeMap<String, Vec<(String, Value)>>;

/// Lists the v2 tasks, and with `--yes` rewrites them as v3.
pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: MigrateInput = PluginError::read_input(PLUGIN, "migrate", input)?;
    let yes = given.yes == Some(true);
    let found = listed(host, PLUGIN)?;
    let mut repos = vec![Repo {
        root: "/".to_owned(),
        clone: None,
    }];
    repos.extend(
        (found.iter())
            .filter(|listed| listed.root != "/")
            .map(|listed| Repo {
                root: listed.root.clone(),
                clone: listed.clone.clone(),
            }),
    );
    let clones = Through::new(host);
    let writer = writer(host, &clones);
    let mut unsynced = BTreeMap::new();
    let mut refused = BTreeMap::new();
    if yes {
        for repo in &repos {
            unsynced.insert(repo.root.clone(), push(writer, repo));
            if let Err(error) = repo.pin(writer) {
                refused.insert(repo.root.clone(), error);
            }
        }
    }
    let roots: Vec<String> = repos.iter().map(|repo| repo.root.clone()).collect();
    let (tasks, projects) = v2_tasks(host, &roots)?;
    if yes && !projects.is_empty() {
        let shown: Vec<&String> = projects.iter().take(SHOWN_PROJECTS).collect();
        let why = format!(
            "{} project tasks are tasks/task/v2, such as {}, and a project stays v2 until projects \
             move to their own records; nothing was written",
            projects.len(),
            (shown.iter().map(|path| path.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Err(hinted(
            "tasks::migrate::project_task",
            why,
            [
                ("projects", json!(shown)),
                ("projects_total", json!(projects.len())),
            ],
            "existing tasks gain refs once projects move to their own records, which a later \
             release adds; until then a new task from `board tasks add` is v3 and takes refs",
        ));
    }
    let unchanged = tasks.values().all(Vec::is_empty);
    let naming = naming(host, &tasks)?;
    let mut reports = Vec::new();
    let mut wrote = false;
    for repo in &repos {
        let listed = tasks.get(&repo.root).map_or(&[][..], Vec::as_slice);
        let (mut commit, mut unpushed) = (None, None);
        if yes && !listed.is_empty() && !refused.contains_key(&repo.root) {
            match rewrite(host, writer, repo, listed) {
                Ok(landing) => {
                    (commit, unpushed) = landing;
                    wrote = true;
                }
                Err(error) => {
                    refused.insert(repo.root.clone(), error);
                }
            }
        }
        reports.push(RepoMigration {
            repo: repo.root.clone(),
            tasks: (listed.iter().take(SHOWN))
                .map(|(path, record)| Migrated {
                    path: path.clone(),
                    record: as_v3(record),
                })
                .collect(),
            total: listed.len(),
            commit,
            unsynced: unsynced.get(&repo.root).cloned().flatten(),
            unpushed,
            refused: refused.remove(&repo.root).map(|error| Refused {
                code: error.code,
                message: error.message,
            }),
        });
    }
    Ok(json!(Output {
        repos: reports,
        projects: projects.iter().take(SHOWN_PROJECTS).cloned().collect(),
        projects_total: projects.len(),
        naming_total: naming.len(),
        naming: naming.into_iter().take(SHOWN_NAMING).collect(),
        wrote,
        unchanged,
    }))
}

/// The v2 tasks in `roots` that are not projects, by their repo's root,
/// and the paths of the v2 project tasks, each in path order.
fn v2_tasks(host: &dyn Host, roots: &[String]) -> Result<(Found, Vec<String>), PluginError> {
    let (_, rows) = listing(host)?;
    let mut tasks: Found = BTreeMap::new();
    let mut projects = Vec::new();
    for [repo, path, _, body] in &rows {
        let at = root(&shown(repo));
        if body.is_null() || !roots.contains(&at) {
            continue;
        }
        let path = shown(path);
        let Ok(record) = json_body(PLUGIN, &path, body) else {
            continue;
        };
        if record.get("$type").and_then(Value::as_str) != Some(V2_TYPE) {
            continue;
        }
        if record.get("kind").and_then(Value::as_str) == Some("project") {
            projects.push(path);
        } else {
            tasks.entry(at).or_default().push((path, record));
        }
    }
    Ok((tasks, projects))
}

/// The questions whose `blocks`, and the artifacts whose `subject`, name a
/// task in `tasks`, by path. A reference is read from the root of its
/// record's own board, so the task's path is the record's mount prefix
/// followed by the reference.
fn naming(host: &dyn Host, tasks: &Found) -> Result<Vec<Naming>, PluginError> {
    let listed: std::collections::BTreeSet<&str> = (tasks.values().flatten())
        .map(|(path, _)| path.as_str())
        .collect();
    if listed.is_empty() {
        return Ok(Vec::new());
    }
    let prefix = "substr(q.logical, 1, length(q.logical) - length('/' || CASE WHEN q.path LIKE \
                  '%.json' THEN substr(q.path, 1, length(q.path) - 5) ELSE q.path END))";
    let sql = format!(
        "SELECT q.logical, {prefix} || json_extract(q.body, '$.blocks') FROM records q \
         WHERE q.type = 'plans/question/v1' AND json_extract(q.body, '$.blocks') IS NOT NULL \
         UNION ALL \
         SELECT q.logical, {prefix} || s.value FROM records q, json_each(q.body, '$.subject') s \
         WHERE q.type = 'artifacts/artifact/v1' \
         ORDER BY 1, 2"
    );
    let report = (host.query(&sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused(PLUGIN, "query", &error.refusal()))?;
    Ok((report.rows.iter())
        .filter_map(|row| Some((row.first()?.as_str()?, row.get(1)?.as_str()?)))
        .filter(|(_, task)| listed.contains(task))
        .map(|(path, task)| Naming {
            path: path.to_owned(),
            task: task.to_owned(),
        })
        .collect())
}

/// `record`, a v2 task, as the v3 record the rewrite makes of it.
fn as_v3(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(fields) = record.as_object_mut() {
        fields.insert("$type".to_owned(), json!(TaskRecord::TYPE));
        fields.remove("kind");
    }
    record
}

/// Rewrites `listed`, the v2 tasks of `repo`, as v3 in one bulk commit and
/// lands it: the commit and the push's code, as [`landed`] gives them.
fn rewrite(
    host: &dyn Host,
    writer: board_repos::Writer<'_>,
    repo: &Repo,
    listed: &[(String, Value)],
) -> Result<(Option<String>, Option<String>), PluginError> {
    let lines = rewritten(host, listed)?;
    if lines.is_empty() {
        return Ok((None, None));
    }
    let refused =
        |error: board_model::Refusal| PluginError::board_refused(PLUGIN, "put --bulk", &error);
    let bulk = Bulk::new(BULK).map_err(|error| refused(error.refusal()))?;
    let report = (host.put_bulk(&lines, &bulk)).map_err(|error| refused(error.refusal()))?;
    let made = report.commit.map(|commit| commit.to_string());
    landed(writer, repo, made)
}

/// The `put --batch` lines that rewrite each of `listed` as v3, each
/// expecting the blob read.
fn rewritten(host: &dyn Host, listed: &[(String, Value)]) -> Result<Vec<Value>, PluginError> {
    let paths: Vec<LogicalPath> = (listed.iter())
        .filter_map(|(path, _)| path.parse().ok())
        .collect();
    let refused = |error: &board_ops::ReadError| {
        PluginError::board_refused(PLUGIN, "get", &error.refusal("get"))
    };
    let answers = host.get_each(&paths).map_err(|error| refused(&error))?;
    let values = [("$type", Some(json!(TaskRecord::TYPE))), ("kind", None)];
    let mut lines = Vec::new();
    for (path, answer) in paths.iter().zip(answers) {
        let report = answer.map_err(|error| refused(&error))?;
        let board_model::Object::Blob(blob) = report.location.object else {
            continue;
        };
        let Ok(text) = String::from_utf8(report.bytes) else {
            continue;
        };
        let text = with_fields(&text, &values, path.as_str())?;
        lines.push(json!({"path": path.to_string(), "expect": blob.to_string(), "text": text}));
    }
    Ok(lines)
}

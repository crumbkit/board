//! `migrate`: lists each `tasks/task/v2` task that is not a project and the
//! `tasks/task/v3` record it becomes, and with `--yes` rewrites them, one
//! bulk commit per repo (plugins/tasks/README.md, Migrating to v3).
//!
//! The rewrite sets `$type` and removes `kind`, which a v2 task that is not
//! a project holds only as `task`, and keeps every other byte. A project
//! task stays v2 until projects move to their own records, so while one
//! remains `--yes` refuses and writes nothing.

use std::collections::BTreeMap;

use board_model::LogicalPath;
use board_ops::{Bulk, Host, PluginError};
use board_plugin::shape::Record as _;
use board_repos::{Repo, Through, json_body, landed, listed, push, root, shown};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::lines::with_fields;
use crate::listing::listing;
use crate::records::{TaskRecord, TaskV2Record};
use crate::write::MigrateInput;
use crate::{PLUGIN, hinted, writer};

/// The most tasks a repo's listing names; `total` counts them all.
const SHOWN: usize = 100;
/// The most project tasks a refusal or a listing names.
const SHOWN_PROJECTS: usize = 20;
/// The reason the bulk commit's trailer gives.
const BULK: &str = "tasks migrate to tasks/task/v3";

/// A v2 task and the v3 record it becomes.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Migrated {
    pub(crate) path: String,
    pub(crate) record: Value,
}

/// One repo's v2 tasks: at most 100, by path, and how many there are. With
/// `--yes`, `commit` holds the rewrite, null when nothing was written or the
/// board's own tree left it uncommitted; `unsynced` and `unpushed` are the
/// codes of the sync before and the push after, as a write call's are.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct RepoMigration {
    pub(crate) repo: String,
    pub(crate) tasks: Vec<Migrated>,
    pub(crate) total: usize,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
    pub(crate) unpushed: Option<String>,
}

/// What `migrate` found, and wrote with `--yes`.
///
/// `repos` lists each repo read, `/` first. `projects` names at most 20
/// project tasks, which stay v2, and `projects_total` counts them.
/// `wrote` says whether `--yes` rewrote any task, and `unchanged` that no
/// task was left to rewrite.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "MigrateOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) repos: Vec<RepoMigration>,
    pub(crate) projects: Vec<String>,
    pub(crate) projects_total: usize,
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
    if yes {
        for repo in &repos {
            unsynced.insert(repo.root.clone(), push(writer, repo));
            repo.pin(writer)?;
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
            "run `board projects migrate` to move the project tasks first, then run `board \
             tasks migrate --yes` again",
        ));
    }
    let unchanged = tasks.values().all(Vec::is_empty);
    let mut texts = BTreeMap::new();
    if yes {
        for (root, listed) in &tasks {
            texts.insert(root.clone(), rewritten(host, listed)?);
        }
    }
    let mut reports = Vec::new();
    for repo in &repos {
        let listed = tasks.get(&repo.root).map_or(&[][..], Vec::as_slice);
        let (mut commit, mut unpushed) = (None, None);
        if let Some(lines) = texts.get(&repo.root).filter(|lines| !lines.is_empty()) {
            let bulk = Bulk::new(BULK).map_err(|error| {
                PluginError::board_refused(PLUGIN, "put --bulk", &error.refusal())
            })?;
            let report = (host.put_bulk(lines, &bulk)).map_err(|error| {
                PluginError::board_refused(PLUGIN, "put --bulk", &error.refusal())
            })?;
            let made = report.commit.map(|commit| commit.to_string());
            (commit, unpushed) = landed(writer, repo, made)?;
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
        });
    }
    Ok(json!(Output {
        repos: reports,
        projects: projects.iter().take(SHOWN_PROJECTS).cloned().collect(),
        projects_total: projects.len(),
        wrote: yes && !unchanged,
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
        if record.get("$type").and_then(Value::as_str) != Some(TaskV2Record::TYPE) {
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

/// `record`, a v2 task, as the v3 record the rewrite makes of it.
fn as_v3(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(fields) = record.as_object_mut() {
        fields.insert("$type".to_owned(), json!(TaskRecord::TYPE));
        fields.remove("kind");
    }
    record
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

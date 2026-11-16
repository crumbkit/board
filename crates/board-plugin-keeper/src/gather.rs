//! `gather`: the live facts a consolidation pass needs, from the changes,
//! tasks and plans plugins, the change and task records and the latest map.
//! It writes nothing.

use crate::clock::{self, Date};
use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::host::{Caller, call, get_text, rows, text};
use crate::stale::{ChangeFacts, TaskFacts, stale};
use crate::{LATEST, PLUGIN};

/// What to gather. A caller names the tasks' repo as `tasks.tree` takes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GatherInput")]
pub(crate) struct Input {
    /// One repo's tasks: `/` for the board's own tree, or a notes repo's
    /// mount path such as `/repos/notes`; the caller's git remote chooses
    /// the repos when left out.
    #[schemars(regex(pattern = r"^/"))]
    repo: Option<String>,
}

/// The UTC day of the host's clock.
pub(crate) fn today(host: &dyn Host) -> Date {
    clock::utc(host.settings().now).0
}

/// The open changes of `changes.open`, as the stale rules and the map check
/// read them, with the day each was touched.
pub(crate) fn open_changes(
    host: &dyn Host,
    caller: Caller,
) -> Result<Vec<ChangeFacts>, PluginError> {
    let listed = call(host, caller, "changes.open", &json!({"limit": 10_000}))?;
    let mut touched = std::collections::BTreeMap::new();
    let mut plans = std::collections::BTreeMap::new();
    let sql = "SELECT logical, json_extract(body, '$.touched'), json_extract(body, '$.plan') \
               FROM records WHERE type IN ('changes/change/v1', 'changes/change/v2')";
    for row in rows(host, sql)? {
        let Some(path) = text(row.first()) else {
            continue;
        };
        if let Some(day) = text(row.get(1)) {
            touched.insert(path.to_owned(), day.to_owned());
        }
        if let Some(plan) = text(row.get(2)) {
            plans.insert(path.to_owned(), plan.to_owned());
        }
    }
    let field = |change: &Value, name: &str| change[name].as_str().map(str::to_owned);
    Ok((listed["changes"].as_array().into_iter().flatten())
        .map(|change| {
            let path = field(change, "path").unwrap_or_default();
            ChangeFacts {
                title: field(change, "title").unwrap_or_default(),
                phase: field(change, "phase").unwrap_or_default(),
                prototype: change["kind"] == "prototype",
                verdict: field(&change["prototype"], "verdict"),
                task: field(change, "task"),
                mount: field(change, "mount"),
                plan: plans.get(&path).cloned(),
                touched: touched.get(&path).cloned(),
                path,
            }
        })
        .collect())
}

/// Every open task under `roots`, depth first, by path.
fn open_tasks(roots: &Value, found: &mut Vec<(String, String)>) {
    for node in roots.as_array().into_iter().flatten() {
        if node["status"] == "open"
            && let (Some(path), Some(title)) = (node["path"].as_str(), node["title"].as_str())
        {
            found.push((path.to_owned(), title.to_owned()));
        }
        open_tasks(&node["children"], found);
    }
}

/// The open tasks of `tasks.tree`, with the day each was last looked at,
/// and how many top-level trees the tree capability left out.
pub(crate) fn tasks(
    host: &dyn Host,
    repo: Option<&str>,
) -> Result<(Vec<TaskFacts>, usize), PluginError> {
    let mut input = Map::from_iter([("root".to_owned(), json!(true))]);
    if let Some(repo) = repo {
        input.insert("repo".to_owned(), json!(repo));
    }
    let tree = call(host, Caller::Gather, "tasks.tree", &Value::Object(input))?;
    let mut open = Vec::new();
    open_tasks(&tree["roots"], &mut open);
    let mut reviewed = std::collections::BTreeMap::new();
    let sql = "SELECT logical, json_extract(body, '$.reviewed') FROM records \
               WHERE type = 'tasks/task/v2' AND json_extract(body, '$.status') = 'open'";
    for row in rows(host, sql)? {
        if let (Some(path), Some(day)) = (text(row.first()), text(row.get(1))) {
            reviewed.insert(path.to_owned(), day.to_owned());
        }
    }
    let left_out =
        tree["total"]
            .as_u64()
            .zip(tree["roots"].as_array())
            .map_or(0, |(total, roots)| {
                usize::try_from(total)
                    .unwrap_or(usize::MAX)
                    .saturating_sub(roots.len())
            });
    let facts = (open.into_iter())
        .map(|(path, title)| TaskFacts {
            reviewed: reviewed.get(&path).cloned(),
            path,
            title,
        })
        .collect();
    Ok((facts, left_out))
}

/// The latest map's date and the paths of the changes it places, or none.
fn latest_map(host: &dyn Host) -> Result<Option<(String, Vec<String>)>, PluginError> {
    let Some(text) = get_text(host, LATEST)? else {
        return Ok(None);
    };
    let Ok(map) = serde_json::from_str::<Value>(&text) else {
        return Ok(None);
    };
    let placed = (map["projects"].as_array().into_iter().flatten())
        .flat_map(|project| project["changes"].as_array().into_iter().flatten())
        .filter_map(|path| path.as_str().map(str::to_owned))
        .collect();
    Ok(Some((
        map["date"].as_str().unwrap_or_default().to_owned(),
        placed,
    )))
}

pub(crate) fn gather(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { repo } = PluginError::read_input(PLUGIN, "gather", input)?;
    let today = today(host);
    let changes = open_changes(host, Caller::Gather)?;
    let (tasks, trees_left_out) = tasks(host, repo.as_deref())?;
    let questions = call(
        host,
        Caller::Gather,
        "plans.open",
        &json!({"limit": 10_000}),
    )?;
    let map = latest_map(host)?;
    let unplaced: Vec<&str> = map.as_ref().map_or(Vec::new(), |(_, placed)| {
        (changes.iter())
            .map(|change| change.path.as_str())
            .filter(|path| !placed.iter().any(|each| each == path))
            .collect()
    });
    let stale = stale(today, &changes, &tasks);
    let changes_json: Vec<Value> = (changes.iter())
        .map(|change| {
            json!({
                "path": change.path, "title": change.title, "phase": change.phase,
                "kind": if change.prototype { "prototype" } else { "build" },
                "task": change.task, "touched": change.touched,
            })
        })
        .collect();
    let tasks_json: Vec<Value> = (tasks.iter())
        .map(|task| json!({"path": task.path, "title": task.title, "reviewed": task.reviewed}))
        .collect();
    Ok(json!({
        "today": today.to_string(),
        "changes": changes_json,
        "tasks": {"open": tasks_json, "trees_left_out": trees_left_out},
        "questions": {
            "waiting": questions["questions"],
            "total": questions["total"],
            "truncated": questions["truncated"],
            "unreadable": questions["unreadable"],
        },
        "map": map.map(|(date, placed)| json!({"path": LATEST, "date": date, "changes_placed": placed.len()})),
        "unplaced": unplaced,
        "stale": stale,
    }))
}

//! `render`: reads the latest map and the live records, builds the page's
//! three data blocks and writes the page.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::gather::open_changes;
use crate::host::{Caller, get_text, put, rows, text};
use crate::model::{self, ChangeFact, Facts, Pass, Workspace};
use crate::survey::survey;
use crate::{LATEST, PLUGIN, refusal};
use board_ops::literal;

/// Where the page goes when the caller names no path.
pub(crate) const DEFAULT_PATH: &str = "/projects/keeper/work-view/index.html";

/// Where to write the page.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RenderInput")]
pub(crate) struct Input {
    /// The file to write, by logical path; `/projects/keeper/work-view/index.html`
    /// when left out.
    #[schemars(regex(pattern = r"^/"))]
    path: Option<String>,
}

/// The latest map as the page reads it.
fn latest(host: &dyn Host) -> Result<Pass, PluginError> {
    let Some(stored) = get_text(host, LATEST)? else {
        return Err(refusal(
            "keeper::render::no_map",
            format!(
                "the board holds no map at {LATEST}; run the keeper skill's consolidation pass"
            ),
            [("path", json!(LATEST))],
        ));
    };
    serde_json::from_str(&stored).map_err(|error| {
        refusal(
            "keeper::render::bad_map",
            format!(
                "the map at {LATEST} does not read: {error}; run the keeper skill's consolidation \
                 pass to write a new one"
            ),
            [("path", json!(LATEST))],
        )
    })
}

/// The text of each change's plan, by the change's path. The page matches
/// them against the crates' names, so none is read when there are no crates.
fn plans(
    host: &dyn Host,
    plan_paths: &[(&str, &str)],
) -> Result<Vec<(String, String)>, PluginError> {
    if plan_paths.is_empty() {
        return Ok(Vec::new());
    }
    let list: Vec<String> = plan_paths.iter().map(|(_, plan)| literal(plan)).collect();
    let sql = format!(
        "SELECT logical, json_extract(body, '$.text') FROM records WHERE logical IN ({})",
        list.join(", ")
    );
    let mut texts = Vec::new();
    for row in rows(host, &sql)? {
        let (Some(plan), Some(file)) = (text(row.first()), text(row.get(1))) else {
            continue;
        };
        if let Some(words) = get_text(host, file)?
            && let Some((change, _)) = plan_paths.iter().find(|(_, each)| *each == plan)
        {
            texts.push(((*change).to_owned(), words));
        }
    }
    Ok(texts)
}

/// The crates of each system the map marks, and for each system that has
/// none, why.
fn workspaces(host: &dyn Host, pass: &Pass) -> Result<(Vec<Workspace>, Vec<Value>), PluginError> {
    let mut found = Vec::new();
    let mut surveys = Vec::new();
    for system in pass.systems.iter().flatten() {
        let Some(mount) = &system.crates_from else {
            continue;
        };
        match survey(host, mount)? {
            Ok(crates) => {
                surveys.push(json!({"system": system.id, "crates": crates.len()}));
                found.push(Workspace {
                    system: system.id.clone(),
                    crates,
                });
            }
            Err(problem) => {
                surveys.push(json!({"system": system.id, "crates": 0, "problem": problem}));
            }
        }
    }
    Ok((found, surveys))
}

/// What the board holds now, what each survey found, and how many open
/// questions `plans.open` could not read.
fn facts(host: &dyn Host, pass: &Pass) -> Result<(Facts, Vec<Value>, u64), PluginError> {
    let open = open_changes(host, Caller::Render)?;
    let (workspaces, surveys) = workspaces(host, pass)?;
    let wanted: Vec<(&str, &str)> = (open.iter())
        .filter(|_| {
            workspaces
                .iter()
                .any(|workspace| !workspace.crates.is_empty())
        })
        .filter_map(|change| Some((change.path.as_str(), change.plan.as_deref()?)))
        .collect();
    let plans = plans(host, &wanted)?;
    let changes = (open.iter())
        .map(|change| ChangeFact {
            path: change.path.clone(),
            title: change.title.clone(),
            phase: change.phase.clone(),
            mount: change.mount.clone(),
            touched: change.touched.clone(),
            task: change.task.clone(),
            plan: (plans.iter())
                .find(|(path, _)| *path == change.path)
                .map(|(_, words)| words.clone()),
        })
        .collect();
    let (questions, unreadable) = crate::map::waiting(host, Caller::Render)?;
    Ok((
        Facts {
            changes,
            questions,
            workspaces,
        },
        surveys,
        unreadable,
    ))
}

pub(crate) fn render(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input(PLUGIN, "render", input)?;
    let path = path.unwrap_or_else(|| DEFAULT_PATH.to_owned());
    let pass = latest(host)?;
    let (facts, surveys, unreadable) = facts(host, &pass)?;
    let page = model::build(&pass, &facts)?;
    put(
        host,
        &[json!({"path": path, "text": crate::page::html(&page)})],
    )?;
    Ok(
        json!({"path": path, "missing": page.data["missing"], "open": page.data["open"], "surveys": surveys, "questions_unreadable": unreadable}),
    )
}

//! The page's data, from the latest map and the live facts. The page's
//! JavaScript turns the three blocks built here into its display model, so
//! this module settles every fact that has one right answer: which change
//! each map path is, which question counts where, and what each total is.
//! Every list is sorted by a stated rule and every object has sorted keys, so
//! the order of the inputs never shows in the output.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::PluginError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::placement::{self, Placed, Tree, Waiting, change_id};
use crate::records::{Finding, Link, LinkKind, Project, QuestionRules, Realm, System};
use crate::refusal;

/// The map record, as far as the page reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct Pass {
    pub(crate) date: String,
    pub(crate) realms: Vec<Realm>,
    pub(crate) projects: Vec<Project>,
    pub(crate) findings: Vec<Finding>,
    pub(crate) links: Option<Vec<Link>>,
    pub(crate) systems: Option<Vec<System>>,
    pub(crate) question_rules: Option<QuestionRules>,
}

/// An open change as `changes.open` and its record give it.
#[derive(Debug, Deserialize)]
pub(crate) struct ChangeFact {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) phase: String,
    /// The mount the change is for, such as `/repos/board`.
    pub(crate) mount: Option<String>,
    /// The UTC day a changes capability last touched it.
    pub(crate) touched: Option<String>,
    /// The task it does, by path.
    pub(crate) task: Option<String>,
    /// The text of its plan.
    pub(crate) plan: Option<String>,
}

/// A crate of a Cargo workspace, as the survey reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct CrateFact {
    pub(crate) id: String,
    pub(crate) lines: u64,
    pub(crate) deps: Vec<String>,
    pub(crate) modules: Vec<ModuleFact>,
}

/// A top-level module of a crate.
#[derive(Debug, Deserialize)]
pub(crate) struct ModuleFact {
    pub(crate) id: String,
    pub(crate) lines: u64,
}

/// The crates of one system's Cargo workspace.
#[derive(Debug, Deserialize)]
pub(crate) struct Workspace {
    /// The system's id, which is its mount's name.
    pub(crate) system: String,
    pub(crate) crates: Vec<CrateFact>,
}

/// What the board holds now.
#[derive(Debug, Deserialize)]
pub(crate) struct Facts {
    pub(crate) changes: Vec<ChangeFact>,
    pub(crate) questions: Vec<Waiting>,
    pub(crate) workspaces: Vec<Workspace>,
}

/// The three blocks the page reads, and what the header says about the map.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) data: Value,
    pub(crate) systems: Value,
    pub(crate) questions: Value,
}

/// A day as the page reads it, or null.
fn day(text: Option<&String>) -> Value {
    json!(text)
}

/// The mount's name when the change's mount is a repository directly under
/// `/repos`, which is the id of the system it changes.
fn repo(mount: Option<&String>) -> Value {
    json!(
        mount
            .and_then(|mount| mount.strip_prefix("/repos/"))
            .filter(|name| !name.contains('/'))
    )
}

/// The crates of `names` that `text` names, with no letter, digit or hyphen
/// beside the name.
fn crates_named(text: &str, names: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = (names.iter())
        .filter(|name| placement::names(text, name))
        .map(|name| (*name).to_owned())
        .collect();
    found.sort();
    found
}

/// The kinds of the findings that name `path`, sorted, once each. The three
/// kinds of a plan that needs tidying are among them, and the page marks
/// those.
fn finding_kinds(findings: &[Finding], path: &str) -> Vec<String> {
    let kinds: BTreeSet<String> = (findings.iter())
        .filter(|finding| finding.paths.iter().any(|each| each == path))
        .map(|finding| {
            serde_json::to_value(finding.kind)
                .ok()
                .and_then(|kind| kind.as_str().map(str::to_owned))
                .expect("a kind is a string")
        })
        .collect();
    kinds.into_iter().collect()
}

fn sums_disagree(realm: &str, counted: usize, listed: usize) -> PluginError {
    refusal(
        "keeper::render::sums_disagree",
        format!(
            "the realm {realm} counts {counted} waiting questions but its rows and its line of \
             questions tied to no project add up to {listed}; this is a bug in the placement \
             rules or the code"
        ),
        [("realm", json!(realm))],
    )
}

/// The page's `questions` block, with each realm's total checked against
/// the sum of its rows and its line of unplaced questions.
fn questions_block(
    pass: &Pass,
    live: &BTreeMap<&str, &ChangeFact>,
    questions: &[Waiting],
) -> Result<Value, PluginError> {
    let Some(rules) = &pass.question_rules else {
        return Ok(json!({"asOf": null, "projects": {}, "unassigned": {}}));
    };
    let mut owner = BTreeMap::new();
    for project in &pass.projects {
        for path in project
            .changes
            .iter()
            .filter(|path| live.contains_key(path.as_str()))
        {
            owner.insert(change_id(path).to_owned(), project.id.clone());
        }
    }
    let realm_of = (pass.projects.iter())
        .map(|project| (project.id.clone(), project.realm.clone()))
        .collect();
    let placed = placement::place(
        &Tree {
            owner,
            realm_of,
            rules,
        },
        questions,
    )?;
    let mut by_project: BTreeMap<&str, Vec<&Placed>> = BTreeMap::new();
    let mut unassigned: BTreeMap<&str, Vec<&Placed>> = BTreeMap::new();
    for question in &placed {
        match &question.project {
            Some(project) => by_project.entry(project).or_default().push(question),
            None => unassigned
                .entry(&question.realm)
                .or_default()
                .push(question),
        }
    }
    for realm in &pass.realms {
        let counted = placed
            .iter()
            .filter(|question| question.realm == realm.id)
            .count();
        let rows: usize = (pass.projects.iter())
            .filter(|project| project.realm == realm.id)
            .map(|project| by_project.get(project.id.as_str()).map_or(0, Vec::len))
            .sum();
        let listed = rows + unassigned.get(realm.id.as_str()).map_or(0, Vec::len);
        if counted != listed {
            return Err(sums_disagree(&realm.id, counted, listed));
        }
    }
    Ok(json!({
        "asOf": null,
        "projects": by_project,
        "unassigned": unassigned,
    }))
}

fn kind_word(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::ApiCall => "road",
        LinkKind::EventStream => "rail",
        LinkKind::LogsAndRecords => "river",
    }
}

/// One crate as the page reads it: its lines, the workspace crates it
/// depends on and its top-level modules, all sorted.
fn crate_block(each: &CrateFact) -> Value {
    let mut deps = each.deps.clone();
    deps.sort();
    let mut modules: Vec<Value> = (each.modules.iter())
        .map(|module| json!({"id": module.id, "loc": module.lines}))
        .collect();
    modules.sort_by_key(|module| module["id"].as_str().unwrap_or_default().to_owned());
    json!({"id": each.id, "loc": each.lines, "deps": deps, "modules": modules})
}

/// The `systems` block: surveyed systems with modules as cities, systems
/// outside the repositories, and every link, sorted by id.
fn systems_block(systems: &[System]) -> Value {
    let key = |value: &Value| value["id"].as_str().unwrap_or_default().to_owned();
    let mut cities = Vec::new();
    let mut external = Vec::new();
    let mut links = Vec::new();
    for system in systems {
        let id = &system.id;
        for link in system.links.iter().flatten() {
            let from = link
                .module
                .as_ref()
                .map_or_else(|| id.clone(), |module| format!("{id}.{module}"));
            let controls: Vec<Value> = (link.controls.iter().flatten())
                .map(|control| {
                    json!({
                        "caption": control.caption, "detail": control.detail,
                        "at": serde_json::to_value(control.at).expect("an end is a string"),
                    })
                })
                .collect();
            links.push(json!({
                "id": link.id, "name": link.name, "kind": kind_word(link.kind),
                "traffic": link.traffic.unwrap_or(1), "prod": link.production.unwrap_or(false),
                "from": from, "to": link.to, "symbols": controls,
            }));
        }
        if system.external == Some(true) {
            external.push(json!({"id": id, "title": system.title}));
        } else if let Some(modules) = system
            .modules
            .as_ref()
            .filter(|modules| !modules.is_empty())
        {
            let districts: Vec<Value> = (modules.iter())
                .map(|module| {
                    json!({
                        "id": format!("{id}.{}", module.id), "title": module.id,
                        "loc": module.lines, "purpose": module.purpose,
                    })
                })
                .collect();
            let mut districts = districts;
            districts.sort_by_key(key);
            cities.push(json!({
                "id": id, "realm": system.realm, "title": system.title,
                "lang": system.language, "districts": districts,
            }));
        }
    }
    cities.sort_by_key(key);
    external.sort_by_key(key);
    links.sort_by_key(key);
    json!({"cities": cities, "links": links, "external": external})
}

/// The newest day among the map's date, the changes' touched days and the
/// questions' days.
pub(crate) fn page_as_of(pass: &Pass, facts: &Facts) -> String {
    let days = std::iter::once(pass.date.as_str())
        .chain(
            facts
                .changes
                .iter()
                .filter_map(|change| change.touched.as_deref()),
        )
        .chain(
            facts
                .questions
                .iter()
                .map(|question| question.asked.as_str()),
        );
    days.filter(|day| day.len() == 10 && day.as_bytes()[4] == b'-')
        .max()
        .unwrap_or(&pass.date)
        .to_owned()
}

/// The three blocks of the page. A change the map names that is no longer
/// open is left out; an open change the map does not name is counted in
/// `data.missing`.
pub(crate) fn build(pass: &Pass, facts: &Facts) -> Result<Page, PluginError> {
    let live: BTreeMap<&str, &ChangeFact> = facts
        .changes
        .iter()
        .map(|change| (change.path.as_str(), change))
        .collect();
    let mut placed = BTreeSet::new();
    let mut projects: Vec<Value> = Vec::new();
    for project in &pass.projects {
        let mut changes: Vec<Value> = Vec::new();
        for path in project
            .changes
            .iter()
            .filter_map(|path| live.get_key_value(path.as_str()))
        {
            let (path, change) = (*path.0, path.1);
            placed.insert(path);
            let mut tasks: Vec<&str> = change.task.iter().map(|task| change_id(task)).collect();
            tasks.sort_unstable();
            let repo = repo(change.mount.as_ref());
            let crate_ids: Vec<&str> = (facts.workspaces.iter())
                .filter(|workspace| repo.as_str() == Some(workspace.system.as_str()))
                .flat_map(|workspace| workspace.crates.iter().map(|each| each.id.as_str()))
                .collect();
            changes.push(json!({
                "id": change_id(path), "title": change.title, "repo": repo,
                "state": change.phase, "date": day(change.touched.as_ref()),
                "findings": finding_kinds(&pass.findings, path),
                "crates": crates_named(change.plan.as_deref().unwrap_or_default(), &crate_ids),
                "tasks": tasks,
            }));
        }
        changes.sort_by_key(|change| change["id"].as_str().unwrap_or_default().to_owned());
        let mut order: Vec<[&str; 2]> = (project.order.iter().flatten())
            .map(|(before, after)| [before.as_str(), after.as_str()])
            .collect();
        order.sort_unstable();
        projects.push(json!({
            "id": project.id, "realm": project.realm, "title": project.title,
            "state": serde_json::to_value(project.state).expect("a state is a string"),
            "summary": project.summary, "changes": changes, "order": order,
        }));
    }
    projects.sort_by_key(|project| project["id"].as_str().unwrap_or_default().to_owned());
    let mut realms: Vec<Value> = (pass.realms.iter())
        .map(|realm| json!({"id": realm.id, "title": realm.title}))
        .collect();
    realms.sort_by_key(|realm| realm["id"].as_str().unwrap_or_default().to_owned());
    let mut links: Vec<Value> = (pass.links.iter().flatten())
        .map(|link| json!({"before": link.before, "after": link.after, "project": link.project}))
        .collect();
    links.sort_by_key(|link| {
        (
            link["before"].as_str().unwrap_or_default().to_owned(),
            link["after"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let mut workspaces: Vec<Value> = (facts.workspaces.iter())
        .map(|workspace| {
            let system =
                (pass.systems.iter().flatten()).find(|system| system.id == workspace.system);
            let mut crates: Vec<Value> = (workspace.crates.iter()).map(crate_block).collect();
            crates.sort_by_key(|each| each["id"].as_str().unwrap_or_default().to_owned());
            json!({
                "id": workspace.system, "crates": crates,
                "title": system.map_or(workspace.system.as_str(), |system| system.title.as_str()),
                "realm": system.map(|system| system.realm.as_str()),
                "lang": system.and_then(|system| system.language.as_deref()),
            })
        })
        .collect();
    workspaces.sort_by_key(|each| each["id"].as_str().unwrap_or_default().to_owned());
    let missing = live.keys().filter(|path| !placed.contains(**path)).count();
    let as_of = page_as_of(pass, facts);
    let mut questions = questions_block(pass, &live, &facts.questions)?;
    questions["asOf"] = json!(as_of);
    Ok(Page {
        data: json!({
            "date": pass.date, "realms": realms, "projects": projects, "links": links,
            "workspaces": workspaces, "open": live.len(), "missing": missing,
        }),
        systems: systems_block(pass.systems.as_deref().unwrap_or_default()),
        questions,
    })
}

#[cfg(test)]
mod tests;

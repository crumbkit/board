//! `map`: checks a consolidation pass's project tree against the board and
//! writes it as a `keeper/map/v1` record at a timestamped path and at
//! `latest`, in one commit. The record's date, its findings' ids, its
//! `dedupe` and its `questions_waiting` are computed here, so the agent
//! cannot get them wrong.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::gather::{open_changes, today};
use crate::host::{blob_at, call, put, rows, text};
use crate::placement::{self, Tree, Waiting};
use crate::records::{
    Finding, KeeperMap, Kind, Link, NewFinding, Project, QuestionRules, Realm, System,
};
use crate::{LATEST, PLUGIN, refusal};

/// A pass's tree and findings. Each path is the one `gather` listed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MapInput")]
pub(crate) struct Input {
    /// The curation record this map was built from.
    #[schemars(with = "board_plugin::shape::Ref")]
    source: String,
    realms: Vec<Realm>,
    projects: Vec<Project>,
    /// Findings without ids; `map` computes each id.
    findings: Vec<NewFinding>,
    links: Option<Vec<Link>>,
    systems: Option<Vec<System>>,
    question_rules: Option<QuestionRules>,
}

/// The paths of the board that the map's paths must be among.
pub(crate) struct Known {
    /// The open changes.
    pub(crate) open: BTreeSet<String>,
    /// Every change, open or not.
    pub(crate) changes: BTreeSet<String>,
}

fn invalid(what: &str, message: String, paths: &[&str]) -> PluginError {
    refusal(
        &format!("keeper::map::{what}"),
        message,
        [("values", json!(paths))],
    )
}

/// Whether findings of `kind` name changes, which must exist.
fn names_changes(kind: Kind) -> bool {
    !matches!(
        kind,
        Kind::LooseLink | Kind::DuplicateMount | Kind::AnsweredOpen
    )
}

/// The first of `ids` that repeats.
fn repeated<'a>(ids: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    ids.into_iter().find(|id| !seen.insert(*id))
}

/// The realms and projects that a map lists.
struct Listed<'a> {
    realms: BTreeSet<&'a str>,
    projects: BTreeSet<&'a str>,
}

impl Listed<'_> {
    fn realm(&self, id: &str, whose: &str) -> Result<(), PluginError> {
        self.realms.contains(id).then_some(()).ok_or_else(|| {
            invalid(
                "unknown_realm",
                format!(
                    "{whose} is in the realm {id}, which the map does not list; list it or use \
                     one it lists"
                ),
                &[id],
            )
        })
    }

    fn project(&self, id: &str, whose: &str) -> Result<(), PluginError> {
        self.projects.contains(id).then_some(()).ok_or_else(|| {
            invalid(
                "unknown_project",
                format!(
                    "{whose} names the project {id}, which the map does not list; list it or \
                     use one it lists"
                ),
                &[id],
            )
        })
    }
}

impl Known {
    fn open(&self, path: &str, whose: &str) -> Result<(), PluginError> {
        self.open.contains(path).then_some(()).ok_or_else(|| {
            invalid(
                "unknown_path",
                format!(
                    "{whose} names {path}, which is no open change; run keeper.gather for the \
                     open changes"
                ),
                &[path],
            )
        })
    }
}

/// Refuses a map whose tree does not match the board.
pub(crate) fn check(input: &Input, known: &Known) -> Result<(), PluginError> {
    for (what, ids) in [
        (
            "realm",
            input
                .realms
                .iter()
                .map(|realm| realm.id.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "project",
            input
                .projects
                .iter()
                .map(|project| project.id.as_str())
                .collect(),
        ),
    ] {
        if let Some(id) = repeated(ids.into_iter()) {
            return Err(invalid(
                "duplicate_id",
                format!("the {what} {id} is listed twice; list each {what} once"),
                &[id],
            ));
        }
    }
    let listed = Listed {
        realms: input.realms.iter().map(|realm| realm.id.as_str()).collect(),
        projects: input
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect(),
    };
    check_projects(input, known, &listed)?;
    for link in input.links.iter().flatten() {
        listed.project(&link.project, "a link")?;
        known.open(&link.before, "a link")?;
        known.open(&link.after, "a link")?;
    }
    for finding in input
        .findings
        .iter()
        .filter(|finding| names_changes(finding.kind))
    {
        if let Some(path) = finding
            .paths
            .iter()
            .find(|path| !known.changes.contains(*path))
        {
            return Err(invalid(
                "unknown_path",
                format!(
                    "a finding of kind {:?} names {path}, which is no change; name a change that \
                     keeper.gather listed",
                    finding.kind
                ),
                &[path],
            ));
        }
    }
    for system in input.systems.iter().flatten() {
        listed.realm(&system.realm, &format!("the system {}", system.id))?;
    }
    if let Some(rules) = &input.question_rules {
        for id in (rules.names.values()).chain(rules.tasks.iter().map(|rule| &rule.to)) {
            listed.project(id, "a question rule")?;
        }
        for id in (rules.mount_realms.values()).chain(rules.local_realm.values()) {
            listed.realm(id, "a question rule")?;
        }
        listed.realm(&rules.local_default, "a question rule")?;
    }
    Ok(())
}

/// Each project's realm, changes and order, and that every open change is
/// in one project.
fn check_projects(input: &Input, known: &Known, listed: &Listed<'_>) -> Result<(), PluginError> {
    let mut placed: BTreeMap<&str, &str> = BTreeMap::new();
    for project in &input.projects {
        let whose = format!("the project {}", project.id);
        listed.realm(&project.realm, &whose)?;
        for path in &project.changes {
            known.open(path, &whose)?;
            if let Some(first) = placed.insert(path, &project.id) {
                return Err(invalid(
                    "duplicate",
                    format!(
                        "{path} is in the projects {first} and {}; place a change in one project",
                        project.id
                    ),
                    &[path],
                ));
            }
        }
        for path in project
            .order
            .iter()
            .flatten()
            .flat_map(|(first, second)| [first, second])
        {
            if !project.changes.contains(path) {
                return Err(invalid(
                    "unknown_path",
                    format!(
                        "the order of the project {} names {path}, which is not one of its \
                         changes; order only its own changes",
                        project.id
                    ),
                    &[path],
                ));
            }
        }
    }
    match known
        .open
        .iter()
        .find(|path| !placed.contains_key(path.as_str()))
    {
        Some(path) => Err(invalid(
            "unplaced",
            format!("the open change {path} is in no project; place it in one"),
            &[path],
        )),
        None => Ok(()),
    }
}

/// A finding's id: its kind, a dash and the first 8 hex digits of the
/// sha256 of its paths joined by newlines.
pub(crate) fn finding_id(kind: Kind, paths: &[String]) -> String {
    let kind = serde_json::to_value(kind).expect("a kind is a string");
    let digest = Sha256::digest(paths.join("\n").as_bytes());
    let hex = format!(
        "{:08x}",
        u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
    );
    format!("{}-{hex}", kind.as_str().expect("a kind is a string"))
}

/// Each path that reaches a commit through a mount inside another mount,
/// with the shortest path that reaches the same commit. `mounts` is each
/// mount's path and the commit it shows.
pub(crate) fn dedupe(mounts: &[(String, String)]) -> BTreeMap<String, String> {
    let mut by_commit: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, commit) in mounts {
        by_commit.entry(commit).or_default().push(path);
    }
    let mut found = BTreeMap::new();
    for paths in by_commit.values_mut() {
        paths.sort_by_key(|path| (path.len(), *path));
        if let Some((first, rest)) = paths.split_first() {
            for path in rest {
                found.insert((*path).to_owned(), (*first).to_owned());
            }
        }
    }
    found
}

/// The path and commit of every mount, as the board's index lists them.
fn mounts(host: &dyn Host) -> Result<Vec<(String, String)>, PluginError> {
    let sql = "SELECT e.logical, m.target FROM mounts m \
               JOIN entries e ON e.source = m.source AND e.path = m.path \
               WHERE m.target IS NOT NULL AND e.logical IS NOT NULL";
    Ok(rows(host, sql)?
        .iter()
        .filter_map(|row| Some((text(row.first())?.to_owned(), text(row.get(1))?.to_owned())))
        .collect())
}

/// Every change record's path.
fn all_changes(host: &dyn Host) -> Result<BTreeSet<String>, PluginError> {
    let sql = "SELECT logical FROM records \
               WHERE type IN ('changes/change/v1', 'changes/change/v2')";
    Ok(rows(host, sql)?
        .iter()
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect())
}

/// The waiting questions of `plans.open` in the shape placement reads.
fn waiting(host: &dyn Host) -> Result<Vec<Waiting>, PluginError> {
    let listed = call(host, "plans.open", &json!({"limit": 10_000}))?;
    let field =
        |question: &Value, name: &str| question[name].as_str().unwrap_or_default().to_owned();
    Ok((listed["questions"].as_array().into_iter().flatten())
        .map(|question| Waiting {
            path: field(question, "path"),
            asked: field(question, "asked").chars().take(10).collect(),
            text: field(question, "question"),
            blocked_work: field(question, "blocked_work"),
        })
        .collect())
}

/// How many questions each realm has waiting, with every realm listed.
fn counts(
    input: &Input,
    rules: &QuestionRules,
    waiting: &[Waiting],
) -> Result<BTreeMap<String, u64>, PluginError> {
    let owner = (input.projects.iter())
        .flat_map(|project| {
            (project.changes.iter())
                .map(|path| (placement::change_id(path).to_owned(), project.id.clone()))
        })
        .collect();
    let realm_of = (input.projects.iter())
        .map(|project| (project.id.clone(), project.realm.clone()))
        .collect();
    let tree = Tree {
        owner,
        realm_of,
        rules,
    };
    let mut counts: BTreeMap<String, u64> = input
        .realms
        .iter()
        .map(|realm| (realm.id.clone(), 0))
        .collect();
    for question in placement::place(&tree, waiting)? {
        *counts.entry(question.realm).or_default() += 1;
    }
    Ok(counts)
}

/// The UTC time as `YYYYMMDDTHHMMSSZ`.
fn stamp(host: &dyn Host) -> String {
    let now = jiff::Timestamp::try_from(host.settings().now)
        .expect("the host's clock is a time that a timestamp holds");
    now.strftime("%Y%m%dT%H%M%SZ").to_string()
}

pub(crate) fn map(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let input: Input = PluginError::read_input(PLUGIN, "map", input)?;
    let open = open_changes(host)?;
    let known = Known {
        open: open.into_iter().map(|change| change.path).collect(),
        changes: all_changes(host)?,
    };
    check(&input, &known)?;
    let findings: Vec<Finding> = (input.findings.iter())
        .map(|finding| Finding {
            kind: finding.kind,
            paths: finding.paths.clone(),
            evidence: finding.evidence.clone(),
            fix: finding.fix.clone(),
            question: finding.question.clone(),
            id: finding_id(finding.kind, &finding.paths),
        })
        .collect();
    let mut record = json!({
        "$type": KeeperMap::TYPE,
        "date": today(host).to_string(),
        "source": input.source,
        "realms": input.realms,
        "projects": input.projects,
        "findings": findings,
    });
    let object = record.as_object_mut().expect("the record is an object");
    let mut optional = |name: &str, value: Option<Value>| {
        if let Some(value) = value {
            object.insert(name.to_owned(), value);
        }
    };
    optional("links", input.links.as_ref().map(to_value));
    optional("systems", input.systems.as_ref().map(to_value));
    optional(
        "question_rules",
        input.question_rules.as_ref().map(to_value),
    );
    let dedupe = dedupe(&mounts(host)?);
    optional("dedupe", (!dedupe.is_empty()).then(|| json!(dedupe)));
    let waiting_counts = match &input.question_rules {
        Some(rules) => Some(counts(&input, rules, &waiting(host)?)?),
        None => None,
    };
    optional(
        "questions_waiting",
        waiting_counts.as_ref().map(|counts| json!(counts)),
    );
    let path = format!("/projects/keeper/map/{}", stamp(host));
    let held = blob_at(host, LATEST)?;
    put(
        host,
        &[
            json!({"path": path, "expect": "none", "record": record}),
            json!({"path": LATEST, "expect": held, "record": record}),
        ],
    )?;
    Ok(json!({
        "path": path,
        "latest": LATEST,
        "projects": input.projects.len(),
        "findings": findings.iter().map(|finding| &finding.id).collect::<Vec<_>>(),
        "questions_waiting": waiting_counts,
    }))
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("a record struct serialises")
}

#[cfg(test)]
mod tests;

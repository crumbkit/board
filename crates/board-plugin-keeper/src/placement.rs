//! Which project a waiting question belongs to, by the owner's rules in the
//! map record. The first rule that matches wins:
//!
//! 1. The question's text or `blocked_work` names the id of an open change,
//!    with word boundaries, and every change it names is in one project.
//! 2. Its `blocked_work` contains the text of a task rule.
//! 3. The `/projects/<name>/` or `/repos/<mount>/projects/<name>/` segment of
//!    its path is tied to a project by `names`.
//!
//! A question no rule places is counted on its realm's line "Not tied to a
//! project". The realm comes from `mount_realms` for a mounted board, else
//! from `localRealm`, else `localDefault`.

use std::collections::BTreeMap;

use board_ops::PluginError;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::records::QuestionRules;
use crate::refusal;

/// An open question that no answer names, as `plans.open` lists it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Waiting {
    pub(crate) path: String,
    /// The day it was asked, `YYYY-MM-DD`.
    pub(crate) asked: String,
    pub(crate) text: String,
    pub(crate) blocked_work: String,
}

/// The rule that placed a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum By {
    Change,
    Task,
    Name,
}

/// A question with the project and realm that count it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Placed {
    pub(crate) id: String,
    pub(crate) asked: String,
    pub(crate) text: String,
    /// The rule that placed it, when one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) by: Option<By>,
    /// The project it counts under, when a rule placed it.
    #[serde(skip)]
    pub(crate) project: Option<String>,
    #[serde(skip)]
    pub(crate) realm: String,
    /// Where an unplaced question comes from: the mount and project folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<String>,
}

/// What the rules need to know about the map: which project holds each open
/// change, and which realm holds each project.
pub(crate) struct Tree<'a> {
    /// The project of each open change, by the change's id.
    pub(crate) owner: BTreeMap<String, String>,
    /// The realm of each project.
    pub(crate) realm_of: BTreeMap<String, String>,
    pub(crate) rules: &'a QuestionRules,
}

/// The id of a change from its path: the last segment.
pub(crate) fn change_id(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `text` holds `id` with no letter, digit or hyphen on either side.
pub(crate) fn names(text: &str, id: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '-';
    text.match_indices(id).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + id.len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The mount and project folder in a question's path, such as
/// `/repos/team/projects/p/questions/q` giving `team` and `p`.
fn folder(path: &str) -> (Option<&str>, Option<&str>) {
    let (mount, rest) = match path
        .strip_prefix("/repos/")
        .and_then(|rest| rest.split_once('/'))
    {
        Some((mount, rest)) => (Some(mount), rest),
        None => (None, path.trim_start_matches('/')),
    };
    match rest
        .strip_prefix("projects/")
        .and_then(|rest| rest.split_once('/'))
    {
        Some((name, _)) => (mount, Some(name)),
        None => (mount, None),
    }
}

fn unknown(what: &str, value: &str, path: &str) -> PluginError {
    refusal(
        &format!("keeper::map::unknown_{what}"),
        format!(
            "the question rules place {path} on the {what} {value}, which the tree does not \
             have; name a {what} that it has"
        ),
        [("values", json!([value]))],
    )
}

/// Each waiting question with its project and realm, sorted by day and
/// path.
pub(crate) fn place(tree: &Tree<'_>, waiting: &[Waiting]) -> Result<Vec<Placed>, PluginError> {
    let rules = tree.rules;
    let mut placed = Vec::new();
    for question in waiting {
        let (mount, name) = folder(&question.path);
        let text = format!("{}\n{}", question.text, question.blocked_work);
        let mut hit: Vec<&str> = (tree.owner.iter())
            .filter(|(id, _)| names(&text, id))
            .map(|(_, project)| project.as_str())
            .collect();
        hit.sort_unstable();
        hit.dedup();
        let by_task =
            (rules.tasks.iter()).find(|rule| question.blocked_work.contains(&rule.contains));
        let by_name = name.and_then(|name| {
            let key = mount.map_or_else(|| name.to_owned(), |mount| format!("{mount}/{name}"));
            rules.names.get(&key)
        });
        let (project, by) = match (hit.as_slice(), by_task, by_name) {
            ([one], _, _) => (Some((*one).to_owned()), Some(By::Change)),
            (_, Some(rule), _) => (Some(rule.to.clone()), Some(By::Task)),
            (_, _, Some(project)) => (Some(project.clone()), Some(By::Name)),
            _ => (None, None),
        };
        let realm = if let Some(project) = &project {
            (tree.realm_of.get(project))
                .ok_or_else(|| unknown("project", project, &question.path))?
                .clone()
        } else if let Some(mount) = mount {
            (rules.mount_realms.get(mount))
                .ok_or_else(|| unknown("realm", mount, &question.path))?
                .clone()
        } else {
            (name.and_then(|name| rules.local_realm.get(name)))
                .unwrap_or(&rules.local_default)
                .clone()
        };
        let from = project.is_none().then(|| {
            let folder = name.unwrap_or("(no project)");
            mount.map_or_else(|| folder.to_owned(), |mount| format!("{mount}/{folder}"))
        });
        placed.push(Placed {
            id: question.path.clone(),
            asked: question.asked.clone(),
            text: question
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            by,
            project,
            realm,
            from,
        });
    }
    placed.sort_by(|a, b| (&a.asked, &a.id).cmp(&(&b.asked, &b.id)));
    Ok(placed)
}

#[cfg(test)]
mod tests;

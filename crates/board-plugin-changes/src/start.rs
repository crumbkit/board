//! The `start` capability: a new change in `plan`, its plan to fill in, its
//! route and the repository's brief.

use std::fmt::Write as _;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, get};
use crate::change::{line, touch};
use crate::gates::SECTIONS;
use crate::phases::check_target;
use crate::plan::plan_lines;
use crate::records::{ChangeRecord, Phase, Route, TASK_TYPE};
use crate::repo::{CODEOWNERS, DOCS, Rule, ask_first, exists, files, owners, route, suite_in};
use crate::tasks::{add_task, routing};
use crate::{PLUGIN, refusal, to_value};

/// The longest name a title is shortened to.
const NAME_MAX: usize = 60;

/// Create a change in `plan` for a mounted repository.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StartInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The repository's mount, such as /repos/board."
    )]
    mount: String,
    #[schemars(
        regex(pattern = "[A-Za-z0-9]"),
        description = "What the change does, in a few words."
    )]
    title: String,
    #[schemars(
        regex(pattern = "^[a-z0-9][a-z0-9-]{0,59}$"),
        description = "The change's name, so it lives at /changes/<name>; made from the title \
                       when left out."
    )]
    name: Option<String>,
    #[schemars(
        regex(pattern = r"^@\S+$"),
        description = "Whose change this is, as CODEOWNERS names people, such as @luke; sets \
                       the route."
    )]
    handle: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The task the change does, such as \
                       /repos/notes-personal/tasks/<slug>.md; when left out, tasks.add adds one \
                       titled as the change."
    )]
    task: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The notes repo tasks.add writes the new task to, such as \
                       /repos/notes-personal; the caller's git remote chooses it when left out."
    )]
    task_repo: Option<String>,
}

/// The new change and the repository's brief.
///
/// `owners` lists the CODEOWNERS rules from `codeowners`, the file read,
/// and `ask_first` the patterns someone other than `handle` owns. `docs`
/// lists the contributing docs found, to read before designing. `suite` is
/// the check suite they name as a `runs/suite/v1` path, or null.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "StartOutput")]
pub(crate) struct Output {
    path: String,
    plan: String,
    task: String,
    route: Route,
    owners: Vec<Rule>,
    ask_first: Vec<String>,
    codeowners: Option<String>,
    docs: Vec<String>,
    suite: Option<String>,
}

/// A change's name from its title: lowercase words joined by hyphens.
pub(crate) fn name_of(title: &str) -> String {
    let lowered = title.to_lowercase();
    let mut name = String::new();
    let mut gap = false;
    for character in lowered.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if gap && !name.is_empty() {
                name.push('-');
            }
            gap = false;
            name.push(character);
        } else {
            gap = true;
        }
    }
    let cut: String = name.chars().take(NAME_MAX).collect();
    cut.trim_matches('-').to_owned()
}

/// Refuses a change path that already holds a file with
/// `changes::change::exists`.
pub(crate) fn refuse_existing(host: &dyn Host, path: &str) -> Result<(), PluginError> {
    let held = get(host, &[path.to_owned()])?.into_iter().next().flatten();
    if held.is_none() {
        return Ok(());
    }
    Err(refusal(
        "changes::change::exists",
        format!(
            "a change is already at {path}; give another --name, or resume it with `board call \
             changes.next --path {path}`"
        ),
        [("path", json!(path))],
    ))
}

/// The change's task: the one `task` names, checked to be a task, or a new
/// one titled `title`.
fn task_for(host: &dyn Host, asked: &Input, title: &str) -> Result<String, PluginError> {
    if let Some(named) = &asked.task {
        check_target(host, named, &[TASK_TYPE])?;
        return Ok(named.clone());
    }
    let route = routing(host, asked.task_repo.as_deref())?;
    add_task(host, title, route, None)
}

/// Creates the change at `/changes/<name>`, its plan and, unless one is
/// named, its task, and returns the brief.
pub(crate) fn start(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input(PLUGIN, "start", input)?;
    let mount = asked.mount.trim_end_matches('/').to_owned();
    let title = asked.title.clone();
    let name = (asked.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| name_of(&title));
    let path = format!("/changes/{name}");
    exists(host, &mount)?;
    refuse_existing(host, &path)?;
    let codeowners = files(host, &mount, &CODEOWNERS)?;
    let rules = owners(codeowners.first().map_or("", |(_, text)| text.as_str()));
    let asking = ask_first(&rules, asked.handle.as_deref());
    let docs = files(host, &mount, &DOCS)?;
    let mut skeleton = format!("# {title}\n");
    for section in SECTIONS {
        let _ = write!(skeleton, "\n## {section}\n");
    }
    let (record, mut lines) = plan_lines(
        host.settings().now,
        &path,
        &skeleton,
        Vec::new(),
        Vec::new(),
    );
    let plan = format!("{record}.md");
    let mut change = ChangeRecord::new(title.clone(), mount.clone(), Phase::Plan);
    change.plan = Some(record);
    let routed = route(&rules, &asking);
    change.route = Some(routed);
    let task = task_for(host, &asked, &title)?;
    change.task = Some(task.clone());
    lines.push(line(&path, "none", &touch(host, change)));
    board::put(host, &lines)?;
    Ok(to_value(Output {
        path,
        plan,
        task,
        route: routed,
        owners: rules,
        ask_first: asking,
        codeowners: codeowners.first().map(|(path, _)| path.clone()),
        suite: suite_in(host, &mount, &docs)?,
        docs: docs.into_iter().map(|(path, _)| path).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::name_of;

    #[test]
    fn a_name_is_the_titles_lowercase_words_joined_by_hyphens() {
        assert_eq!(
            name_of("Let a suite declare a timeout!"),
            "let-a-suite-declare-a-timeout"
        );
        assert_eq!(name_of("  --Fix  it-- "), "fix-it");
        assert_eq!(name_of(&"a ".repeat(40)).len(), 59);
    }
}

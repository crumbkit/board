//! The `start` capability: a new change in the first phase of the flow it
//! follows, its copy of that flow, its plan to fill in, its route, its
//! check suite, the records it surfaces and the repository's brief.

use std::fmt::Write as _;

use board_ops::{Host, PluginError};
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board::{self, get};
use crate::change::{line, touch};
use crate::controls::on_mount;
use crate::flow::{FlowSummary, chosen, copy_line, refuse_invalid, refuse_without_repository};
use crate::gates::SECTIONS;
use crate::phases::check_target;
use crate::plan::plan_lines;
use crate::pr::quote;
use crate::records::{BOARD_ITSELF, BUILD, ChangeRecord, Kind, Route, SUITE_TYPE, TASK_TYPE};
use crate::repo::{CODEOWNERS, DOCS, Rule, ask_first, exists, files, owners, route, suite_in};
use crate::surfaced::{Surfaced, surfaced};
use crate::tasks::{add_task, routing};
use crate::wip::{Limit, Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

/// The longest name a title is shortened to.
const NAME_MAX: usize = 60;
/// The most obligations `start` lists (STANDARDS 23); it counts the rest.
const OBLIGATIONS_SHOWN: usize = 10;

/// Create a change for a mounted repository.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StartInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The repository's mount, such as /repos/board, or / for a change that \
                       edits only the board's own records, such as a flow-authoring change."
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
                       /repos/notes/tasks/<slug>.md; when left out, tasks.add adds one \
                       titled as the change."
    )]
    task: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The notes repo tasks.add writes the new task to, such as \
                       /repos/notes; the caller's git remote chooses it when left out."
    )]
    task_repo: Option<String>,
    #[schemars(
        regex(pattern = "^(/|[a-z0-9]+(-[a-z0-9]+)*$)"),
        description = "The flow the change follows, such as small: the board's own at \
                       /flows/<name>, a shipped one, or the path of a flow record, such as a \
                       draft at /changes/<name>/draft; when left out, the flow \
                       /flows/repos/<mount name> or /flows/default links to, else development."
    )]
    flow: Option<String>,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The check suite the change's runs belong to, a runs/suite/v1 record; the \
                       one the repository's contributing docs name when left out."
    )]
    suite: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        length(min = 1),
        description = "Why the change starts in build although the WIP limit is reached, for a \
                       flow whose first phase is build; recorded on the change."
    )]
    over_limit: Option<String>,
}

/// An obligation on the mount: the control and its summary.
#[derive(Debug, Serialize, JsonSchema)]
struct Owed {
    control: String,
    summary: String,
}

/// The new change and the repository's brief.
///
/// `flow` names the flow the change follows and its phases, and `phase`
/// the first, where the change starts. `owners` lists the CODEOWNERS rules
/// from `codeowners`, the file read, and `ask_first` the patterns someone
/// other than `handle` owns. `docs` lists the contributing docs found, to
/// read before designing. `suite` is the change's check suite as a
/// `runs/suite/v1` path, or null when none is given and the docs name
/// none. `surfaced` lists the open tasks, decisions and evals on the mount,
/// which the change records and its plan cites or dismisses, and
/// `obligations` at most ten of the obligation controls on the mount, by
/// path, with `obligations_left_out` counting the rest. `warning` is set
/// when a change that starts in build went past the WIP limit. `note` says
/// what a change on `/`, the board itself, leaves out because it has no
/// repository.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "StartOutput")]
pub(crate) struct Output {
    path: String,
    flow: FlowSummary,
    phase: String,
    plan: String,
    task: String,
    route: Route,
    owners: Vec<Rule>,
    ask_first: Vec<String>,
    codeowners: Option<String>,
    docs: Vec<String>,
    suite: Option<String>,
    surfaced: Surfaced,
    obligations: Vec<Owed>,
    obligations_left_out: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<Limit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// What `start` says of a change on the board itself.
const BOARD_NOTE: &str = "The change edits the board itself, so it has no repository: no \
    CODEOWNERS, contributing docs, check suite, diff, worktree or obligations on a mount are read, \
    and its route is unowned.";

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

/// Creates the change at `/changes/<name>`, its copy of its flow, its plan
/// and, unless one is named, its task, and returns the brief.
pub(crate) fn start(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input(PLUGIN, "start", input)?;
    let mount = match asked.mount.trim_end_matches('/') {
        "" => BOARD_ITSELF.to_owned(),
        trimmed => trimmed.to_owned(),
    };
    let itself = mount == BOARD_ITSELF;
    let title = asked.title.clone();
    let name = (asked.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| name_of(&title));
    let path = format!("/changes/{name}");
    exists(host, &mount)?;
    refuse_existing(host, &path)?;
    let flow = chosen(host, &mount, asked.flow.as_deref())?;
    refuse_invalid(&flow, "start")?;
    if itself {
        refuse_without_repository(&flow, &mount)?;
    }
    if let Some(suite) = &asked.suite {
        check_target(host, suite, &[SUITE_TYPE])?;
    }
    let (codeowners, docs) = if itself {
        (Vec::new(), Vec::new())
    } else {
        (
            files(host, &mount, &CODEOWNERS)?,
            files(host, &mount, &DOCS)?,
        )
    };
    let rules = owners(codeowners.first().map_or("", |(_, text)| text.as_str()));
    let asking = ask_first(&rules, asked.handle.as_deref());
    let suite = match &asked.suite {
        Some(suite) => Some(suite.clone()),
        None => suite_in(host, &mount, &docs)?,
    };
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
        None,
    );
    let plan = format!("{record}.md");
    let first = flow.phases.first().map_or("", |phase| phase.name.as_str());
    let mut change = ChangeRecord::new(title.clone(), mount.clone(), first);
    let (copy, copied) = copy_line(&path, "none", &flow);
    lines.push(copied);
    change.flow = Some(copy);
    change.plan = Some(record);
    change.suite.clone_from(&suite);
    let routed = route(&rules, &asking);
    change.route = Some(routed);
    let mut warning = None;
    if first == BUILD {
        let moving = Move {
            over: over_command(&asked),
            reason: asked.over_limit.as_deref(),
            days: active_days(host)?,
        };
        warning = admit(host, &path, &mut change, Kind::Build, &moving)?;
    }
    let task = task_for(host, &asked, &title)?;
    change.task = Some(task.clone());
    let found = surfaced(host, &mount, &task)?;
    change.surfaced = found.paths();
    lines.push(line(&path, "none", &touch(host, change)));
    board::put(host, &lines)?;
    let owed = if itself {
        Vec::new()
    } else {
        on_mount(host, &mount)?
    };
    let obligations_left_out = owed.len().saturating_sub(OBLIGATIONS_SHOWN);
    let obligations = (owed.into_iter().take(OBLIGATIONS_SHOWN))
        .map(|(control, summary)| Owed { control, summary })
        .collect();
    Ok(to_value(Output {
        path,
        phase: first.to_owned(),
        flow: FlowSummary::from(&flow),
        plan,
        task,
        route: routed,
        owners: rules,
        ask_first: asking,
        codeowners: codeowners.first().map(|(path, _)| path.clone()),
        suite,
        docs: docs.into_iter().map(|(path, _)| path).collect(),
        surfaced: found,
        obligations,
        obligations_left_out,
        warning,
        note: itself.then(|| BOARD_NOTE.to_owned()),
    }))
}

/// The `start` command that goes past the WIP limit: the flags given, and
/// `--over_limit`.
fn over_command(asked: &Input) -> String {
    let given = [
        ("mount", Some(&asked.mount)),
        ("title", Some(&asked.title)),
        ("name", asked.name.as_ref()),
        ("handle", asked.handle.as_ref()),
        ("task", asked.task.as_ref()),
        ("task_repo", asked.task_repo.as_ref()),
        ("flow", asked.flow.as_ref()),
        ("suite", asked.suite.as_ref()),
    ];
    let flags: Vec<String> = (given.iter())
        .filter_map(|(flag, value)| Some(format!("--{flag} {}", quote(value.as_ref()?))))
        .collect();
    format!(
        "board call changes.start {} --over_limit '<why>'",
        flags.join(" ")
    )
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

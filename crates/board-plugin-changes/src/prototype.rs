//! The `prototype` and `verdict` capabilities: a change that answers one
//! question, and what it found.

use board_ops::{Host, PluginError};
use board_plugin::shape::{Prose, Tag, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board;
use crate::brief::{self, brief_of};
use crate::change::{branch_of, line, prototype_of, read, touch};
use crate::phases::final_phase;
use crate::plan::{Planned, plan_lines};
use crate::pr::quote;
use crate::records::{BUILD, ChangeRecord, DONE, DROPPED, Kind, PrototypeRecord, is_final};
use crate::repo::exists;
use crate::start::{name_of, refuse_existing};
use crate::tasks::{add_task, close_task};
use crate::wip::{Limit, Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

/// A prototype that answers one question: its task under `parent`, its
/// change in build and its brief.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrototypeInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The repository's mount, such as /repos/board."
    )]
    mount: String,
    #[schemars(
        regex(pattern = r"\S"),
        description = "The question the prototype answers, which is its whole plan."
    )]
    question: String,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The task or project the prototype's task goes under, such as \
                       /repos/notes/tasks/<slug>.md."
    )]
    parent: String,
    #[schemars(
        regex(pattern = "[A-Za-z0-9]"),
        description = "What the prototype is, in a few words; the question when left out."
    )]
    title: Option<String>,
    #[schemars(
        regex(pattern = "^[a-z0-9][a-z0-9-]{0,59}$"),
        description = "The change's name, so it lives at /changes/<name>; made from the title \
                       when left out."
    )]
    name: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        length(min = 1),
        description = "Why the prototype starts although the WIP limit is reached; recorded on \
                       it."
    )]
    over_limit: Option<String>,
}

/// The prototype started: its change, task and branch, and the brief its
/// builder starts from.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PrototypeOutput")]
pub(crate) struct Output {
    path: String,
    task: String,
    branch: String,
    brief: brief::Output,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<Limit>,
}

/// How a verdict ends a prototype.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Ended {
    Settled,
    Abandoned,
}

/// What a prototype found, which ends it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerdictInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The prototype's logical path, such as /changes/step-list."
    )]
    path: String,
    #[schemars(
        description = "settled when it answered its question, abandoned when the owner says so."
    )]
    state: Ended,
    #[schemars(
        regex(pattern = r"\S"),
        description = "What it found, in one sentence."
    )]
    verdict: String,
    #[schemars(
        with = "Option<Url>",
        description = "The URL of the artifact version the verdict means."
    )]
    artifact: Option<String>,
}

/// The prototype ended, and the phase that ended it: done when settled,
/// dropped when abandoned.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct VerdictOutput {
    path: String,
    state: Ended,
    phase: String,
}

/// A prototype's plan: its question, which is all of it.
fn question_plan(title: &str, question: &str) -> String {
    format!("# {title}\n\n## Question\n\n{}\n", question.trim())
}

/// Adds the prototype's task under `parent`, writes its change in build
/// with its question as the plan, and briefs it.
///
/// The task is added before the change is written, so a refused write
/// leaves the task behind; drop it with `board tasks drop`.
pub(crate) fn prototype(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input(PLUGIN, "prototype", input)?;
    let mount = asked.mount.trim_end_matches('/').to_owned();
    let title = asked
        .title
        .clone()
        .unwrap_or_else(|| asked.question.clone());
    let name = (asked.name.clone()).unwrap_or_else(|| name_of(&title));
    let path = format!("/changes/{name}");
    exists(host, &mount)?;
    refuse_existing(host, &path)?;
    let mut change = ChangeRecord::new(title.clone(), mount, BUILD);
    let prototype = PrototypeRecord {
        tag: Tag::default(),
        question: asked.question.clone(),
        verdict: None,
        verdict_artifact: None,
    };
    let given = [
        ("mount", Some(&asked.mount)),
        ("question", Some(&asked.question)),
        ("parent", Some(&asked.parent)),
        ("title", asked.title.as_ref()),
        ("name", asked.name.as_ref()),
    ];
    let flags: Vec<String> = (given.iter())
        .filter_map(|(flag, value)| Some(format!("--{flag} {}", quote(value.as_ref()?))))
        .collect();
    let moving = Move {
        over: format!(
            "board changes prototype {} --over_limit '<why>'",
            flags.join(" ")
        ),
        reason: asked.over_limit.as_deref(),
        days: active_days(host)?,
    };
    let limit = admit(host, &path, &mut change, Kind::Prototype, &moving)?;
    let task = add_task(host, &title, serde_json::Map::new(), Some(&asked.parent))?;
    change.task = Some(task.clone());
    let plan = question_plan(&title, &asked.question);
    let (record, mut lines) =
        plan_lines(host.settings().now, &path, &plan, Planned::default(), None);
    change.plan = Some(record);
    lines.push(line(&path, "none", &touch(host, change)));
    lines.push(json!({"path": format!("{path}/prototype"), "expect": "none", "record": prototype}));
    board::put(host, &lines)?;
    Ok(to_value(Output {
        brief: brief_of(host, &path)?,
        branch: branch_of(&path),
        path,
        task,
        warning: limit,
    }))
}

/// Records what the prototype found, and ends it and its task: done when
/// settled, dropped when abandoned.
pub(crate) fn verdict(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let VerdictInput {
        path,
        state,
        verdict,
        artifact,
    } = PluginError::read_input(PLUGIN, "verdict", input)?;
    let (mut change, blob) = read(host, &path)?;
    let Some((mut prototype, prototype_blob)) = prototype_of(host, &path)? else {
        return Err(refusal(
            "changes::prototype::not_prototype",
            format!(
                "{path} is a build change, and only a prototype takes a verdict; move it on with \
                 `board changes next --path {path}`"
            ),
            [("path", json!(path)), ("kind", json!(Kind::Build))],
        ));
    };
    if is_final(&change.phase) {
        return Err(final_phase(
            &path,
            &change.phase,
            format!(
                "the prototype at {path} is {}, so its verdict is already recorded; start another \
                 with `board changes prototype`",
                change.phase
            ),
        ));
    }
    let settled = state == Ended::Settled;
    prototype.verdict = Some(verdict);
    (if settled { DONE } else { DROPPED }).clone_into(&mut change.phase);
    if artifact.is_some() {
        prototype.verdict_artifact = artifact;
    }
    board::put(
        host,
        &[
            line(&path, &blob, &change),
            json!({"path": format!("{path}/prototype"), "expect": prototype_blob, "record": prototype}),
        ],
    )?;
    if let Some(task) = &change.task {
        close_task(host, task, settled)?;
    }
    Ok(to_value(VerdictOutput {
        path,
        state,
        phase: change.phase,
    }))
}

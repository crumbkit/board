//! The `pause`, `resume` and `reopen` capabilities: moving a change out of
//! build and back into it other than through its gate.

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::{Actor, Person, Prose, Record, SessionId, Slug, Tag};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AdvanceInput, Moved, PathInput, final_phase, over_command, refuse_v1};
use crate::board::{self, rows, text};
use crate::change::{kind_of, line, read, touch, write};
use crate::flow::{Flow, of_change};
use crate::plan::stamp;
use crate::pr::quote;
use crate::records::{BUILD, PAUSED, ReopenRecord, is_final};
use crate::wip::{Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

/// A change to move back to build, why, who moves it, and why it may go
/// past the WIP limit when that is reached.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("anyOf" = [
    {"required": ["session"]}, {"required": ["program"]}, {"required": ["person"]},
    {"required": ["agent"]},
]))]
pub(crate) struct ReopenInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(
        with = "Prose",
        length(min = 1),
        description = "Why the change goes back to build, such as the fixes a review asked \
                       for; recorded in a changes/reopen/v1 under the change and shown by \
                       `next` in build."
    )]
    reason: String,
    #[schemars(
        with = "Option<SessionId>",
        description = "The session that reopens the change, by its local_ id."
    )]
    session: Option<String>,
    #[schemars(
        with = "Option<Slug>",
        description = "The program that reopens the change, such as claude-code."
    )]
    program: Option<String>,
    #[schemars(
        with = "Option<Person>",
        description = "The person who reopens the change, such as ana."
    )]
    person: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        description = "The name the agent that reopens the change was given, such as lead."
    )]
    agent: Option<String>,
    #[schemars(
        with = "Option<Prose>",
        length(min = 1),
        description = "Why the change moves into build although the WIP limit is reached; \
                       recorded on the change."
    )]
    over_limit: Option<String>,
}

pub(crate) fn pause(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "pause", input)?;
    let (mut change, blob) = read(host, &path)?;
    if change.phase != BUILD {
        return Err(refusal(
            "changes::pause::not_building",
            format!(
                "the change at {path} is in {}, and only a change in build is paused; list the \
                 changes in build with `board changes open`",
                change.phase
            ),
            [("path", json!(path)), ("phase", json!(change.phase))],
        ));
    }
    PAUSED.clone_into(&mut change.phase);
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: BUILD.to_owned(),
        to: PAUSED.to_owned(),
        warning: None,
    }))
}

pub(crate) fn resume(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let AdvanceInput { path, over_limit } = PluginError::read_input(PLUGIN, "resume", input)?;
    let (mut change, blob) = read(host, &path)?;
    if change.phase != PAUSED {
        return Err(refusal(
            "changes::resume::not_paused",
            format!(
                "the change at {path} is in {}, and only a paused change is resumed; run `board \
                 changes next --path {path}` for its next step",
                change.phase
            ),
            [("path", json!(path)), ("phase", json!(change.phase))],
        ));
    }
    BUILD.clone_into(&mut change.phase);
    let moving = Move {
        over: over_command("resume", &path),
        reason: over_limit.as_deref(),
        days: active_days(host)?,
    };
    let limit = admit(host, &path, &mut change, kind_of(host, &path)?, &moving)?;
    let change = touch(host, change);
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: PAUSED.to_owned(),
        to: BUILD.to_owned(),
        warning: limit,
    }))
}

/// Moves the change back to the build phase of its flow, when that comes
/// before the phase it stands in, and records why.
pub(crate) fn reopen(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: ReopenInput = PluginError::read_input(PLUGIN, "reopen", input)?;
    let over = over_reopen(&asked);
    let ReopenInput {
        path,
        reason,
        session,
        program,
        person,
        agent,
        over_limit,
    } = asked;
    let (mut change, blob) = read(host, &path)?;
    let Some(by) = Actor::of(session, program, person, agent) else {
        return Err(refusal(
            "changes::reopen::no_actor",
            format!(
                "the reopen of {path} names nobody; give --session, --program, --person or an \
                 --agent name that is not blank"
            ),
            [("path", json!(path))],
        ));
    };
    refuse_v1(&path, &change, "reason for a reopen")?;
    let phase = change.phase.clone();
    if is_final(&phase) {
        return Err(final_phase(
            &path,
            &phase,
            format!(
                "the change at {path} is {phase}, so it cannot go back to build; start a new \
                 change with `board changes start`"
            ),
        ));
    }
    if phase == PAUSED {
        return Err(refusal(
            "changes::phase::paused",
            format!(
                "the change at {path} is paused, so it is not reopened; resume it with `board \
                 changes resume --path {path}`"
            ),
            [("path", json!(path)), ("phase", json!(phase))],
        ));
    }
    let flow = of_change(host, &path, &change)?;
    let names = flow.as_ref().map(Flow::names).unwrap_or_default();
    let build = names.iter().position(|name| name == BUILD);
    let standing = names.iter().position(|name| *name == phase);
    if !matches!((build, standing), (Some(build), Some(standing)) if build < standing) {
        let why = if flow.is_none() {
            "follows no flow, so it has no build phase to go back to".to_owned()
        } else if phase == BUILD {
            "is already in build".to_owned()
        } else {
            format!("is in {phase}, and its flow has no build phase before {phase} to go back to")
        };
        return Err(refusal(
            "changes::reopen::no_build",
            format!(
                "the change at {path} {why}; run `board changes next --path {path}` for its \
                 next step"
            ),
            [
                ("path", json!(path)),
                ("phase", json!(phase)),
                ("phases", json!(names)),
            ],
        ));
    }
    BUILD.clone_into(&mut change.phase);
    let moving = Move {
        over,
        reason: over_limit.as_deref(),
        days: active_days(host)?,
    };
    let limit = admit(host, &path, &mut change, kind_of(host, &path)?, &moving)?;
    let change = touch(host, change);
    let record = ReopenRecord {
        tag: Tag::default(),
        reason,
        from: phase.clone(),
        by,
    };
    let at = format!("{path}/reopens/{}", stamp(host.settings().now));
    board::put(
        host,
        &[
            line(&path, &blob, &change),
            json!({"path": at, "expect": "none", "record": record}),
        ],
    )?;
    Ok(to_value(Moved {
        path,
        from: phase,
        to: BUILD.to_owned(),
        warning: limit,
    }))
}

/// The `reopen` command that goes past the WIP limit: the flags given,
/// quoted for a shell, and `--over_limit`.
fn over_reopen(asked: &ReopenInput) -> String {
    let given = [
        ("path", Some(&asked.path)),
        ("reason", Some(&asked.reason)),
        ("session", asked.session.as_ref()),
        ("program", asked.program.as_ref()),
        ("person", asked.person.as_ref()),
        ("agent", asked.agent.as_ref()),
    ];
    let flags: Vec<String> = (given.iter())
        .filter_map(|(flag, value)| Some(format!("--{flag} {}", quote(value.as_ref()?))))
        .collect();
    format!(
        "board changes reopen {} --over_limit '<why>'",
        flags.join(" ")
    )
}

/// The reason of the newest `changes/reopen/v1` under the change at
/// `path`, by path order, or none when it was never reopened.
pub(crate) fn latest_reason(host: &dyn Host, path: &str) -> Result<Option<String>, PluginError> {
    let folder = format!("{path}/reopens/");
    let sql = format!(
        "SELECT json_extract(body, '$.reason') FROM records WHERE type = {} AND \
         substr(logical, 1, {}) = {} ORDER BY logical DESC LIMIT 1",
        literal(ReopenRecord::TYPE),
        folder.chars().count(),
        literal(&folder)
    );
    let found = rows(host, &sql)?;
    Ok(found
        .first()
        .and_then(|row| text(row.first()))
        .map(str::to_owned))
}

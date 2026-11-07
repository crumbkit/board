//! The `pause`, `resume` and `reopen` capabilities: moving a change out of
//! build and back into it other than through its gate.

use board_ops::{Host, PluginError};
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AdvanceInput, Moved, PathInput, final_phase, over_command, refuse_v1};
use crate::change::{kind_of, read, touch, write};
use crate::flow::{Flow, of_change};
use crate::records::{BUILD, PAUSED, is_final};
use crate::wip::{Move, active_days, admit};
use crate::{PLUGIN, refusal, to_value};

/// A change to move back to build, why, and why it may go past the WIP
/// limit when that is reached.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
                       for; recorded on the change and shown by `next` in build."
    )]
    reason: String,
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
                 changes in build with `board call changes.open`",
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
                 call changes.next --path {path}` for its next step",
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
    let ReopenInput {
        path,
        reason,
        over_limit,
    } = PluginError::read_input(PLUGIN, "reopen", input)?;
    let (mut change, blob) = read(host, &path)?;
    refuse_v1(&path, &change, "reason for a reopen")?;
    let phase = change.phase.clone();
    if is_final(&phase) {
        return Err(final_phase(
            &path,
            &phase,
            format!(
                "the change at {path} is {phase}, so it cannot go back to build; start a new \
                 change with `board call changes.start`"
            ),
        ));
    }
    if phase == PAUSED {
        return Err(refusal(
            "changes::phase::paused",
            format!(
                "the change at {path} is paused, so it is not reopened; resume it with `board \
                 call changes.resume --path {path}`"
            ),
            [("path", json!(path)), ("phase", json!(phase))],
        ));
    }
    let flow = of_change(host, &path, &change)?;
    let names = flow.as_ref().map(Flow::names).unwrap_or_default();
    let build = names.iter().position(|name| name == BUILD);
    let standing = names.iter().position(|name| *name == phase);
    if !matches!((build, standing), (Some(build), Some(standing)) if build < standing) {
        return Err(refusal(
            "changes::reopen::no_build",
            format!(
                "the change at {path} is in {phase}, and its flow has no build phase before \
                 {phase} to go back to; run `board call changes.next --path {path}` for its \
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
    change.reopened = Some(reason);
    let moving = Move {
        over: over_command("reopen", &path),
        reason: over_limit.as_deref(),
        days: active_days(host)?,
    };
    let limit = admit(host, &path, &mut change, kind_of(host, &path)?, &moving)?;
    let change = touch(host, change);
    write(host, &path, &blob, &change)?;
    Ok(to_value(Moved {
        path,
        from: phase,
        to: BUILD.to_owned(),
        warning: limit,
    }))
}

//! Where a run of a change stops, and the `stop` capability that records
//! why it stopped.
//!
//! `changes.run` sets a change's `stop`: `through` a phase of its flow, or
//! `until` one, where `done` counts as the phase after the last. `advance`
//! refuses to move a change out of the stop's phase, or out of any later
//! one, with `changes::run::stop_reached`, so a run stops there even when
//! its agent ignores its brief. `run` clears the stop once a spawned
//! agent has exited. The stop of an inline run stays until the person who
//! started it starts a later run, which replaces it. A `through` stop is reached once the change
//! stands in its phase with no blocking check missing, or stands later; an
//! `until` stop as soon as the change stands in its phase or later.

use std::fmt;

use board_ops::{Host, PluginError};
use board_plugin::shape::{Summary, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::board;
use crate::change::read;
use crate::flow::{Flow, Stop};
use crate::gates::Check;
use crate::phases::{check_target, next_of};
use crate::plan::stamp;
use crate::records::{DONE, Reason, StopRecord};
use crate::{PLUGIN, refusal, to_value};

/// The record type of a question for the owner, which the plans plugin
/// owns.
pub(crate) const QUESTION_TYPE: &str = "plans/question/v1";

impl Stop {
    /// The phase the stop names.
    pub(crate) fn phase(&self) -> &str {
        match self {
            Self::Through(phase) | Self::Until(phase) => phase,
        }
    }

    /// The rule the stop sets, in plain words, for a run's brief.
    pub(crate) fn rule(&self) -> String {
        match self {
            Self::Through(phase) | Self::Until(phase) if phase == DONE => {
                "Work through every phase and advance the change to done.".to_owned()
            }
            Self::Through(phase) => format!(
                "Stop in the {phase} phase once its gate passes: do its work, and do not advance \
                 out of it."
            ),
            Self::Until(phase) => format!(
                "Stop as soon as the change enters the {phase} phase: advance into it, and do \
                 none of its work."
            ),
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Through(phase) => write!(formatter, "through {phase}"),
            Self::Until(phase) => write!(formatter, "until {phase}"),
        }
    }
}

/// Where `name` falls in `flow`'s order: its phase's index, the number of
/// phases for `done`, and none for a name the flow lacks, such as `paused`.
pub(crate) fn position(flow: &Flow, name: &str) -> Option<usize> {
    if name == DONE {
        return Some(flow.phases.len());
    }
    flow.phases.iter().position(|phase| phase.name == name)
}

/// Whether a change standing in `phase` has gone as far as `stop` lets it,
/// where `passes` says whether its gate passes.
pub(crate) fn reached(flow: &Flow, phase: &str, stop: &Stop, passes: bool) -> bool {
    let (Some(at), Some(end)) = (position(flow, phase), position(flow, stop.phase())) else {
        return false;
    };
    match stop {
        Stop::Through(_) => at > end || (at == end && passes),
        Stop::Until(_) => at >= end,
    }
}

/// Whether `stop` holds a change standing in `phase` where it is, so
/// `advance` refuses to move it on.
pub(crate) fn holds(flow: &Flow, phase: &str, stop: &Stop) -> bool {
    matches!(
        (position(flow, phase), position(flow, stop.phase())),
        (Some(at), Some(end)) if at >= end
    )
}

/// `changes::run::stop_reached`: the change at `path`, standing in
/// `phase`, is at or past `stop`.
pub(crate) fn stop_reached(path: &str, phase: &str, stop: &Stop) -> PluginError {
    refusal(
        "changes::run::stop_reached",
        format!(
            "the run of {path} stops {stop}, and the change stands in {phase}, so it does not move \
             on; record why the run ended with `board changes stop --path {path} --reason \
             reached --summary '<one line>'` and end the run. To the person who started the run: \
             you move the stop by starting a later run with --through or --until"
        ),
        [
            ("path", json!(path)),
            ("phase", json!(phase)),
            ("stop", json!(stop)),
        ],
    )
}

/// The stop of a change's run as `next` and `run` show it: the stop and
/// whether the change has reached it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub(crate) struct Shown {
    #[serde(flatten)]
    pub(crate) stop: Stop,
    pub(crate) reached: bool,
}

/// Why a run of a change stopped.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StopInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    path: String,
    #[schemars(description = "Why the run stopped: reached, waiting, refused, budget or error.")]
    reason: Reason,
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The plans/question/v1 the run left for the owner, whose blocks names the \
                       change."
    )]
    question: Option<String>,
    #[schemars(
        with = "Summary",
        description = "What happened, in one line of at most 25 words."
    )]
    summary: String,
}

/// The stop record written, the phase the change stands in, and the ids of
/// the checks missing there that block.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "StopOutput")]
pub(crate) struct Output {
    pub(crate) stop: String,
    pub(crate) phase: String,
    pub(crate) missing: Vec<String>,
}

/// Writes a `changes/stop/v1` for the change at `path` with `reason`,
/// `question` and `summary`, and the phase and missing checks `next`
/// reports. It leaves the change's `stop` as it is, so an agent that
/// records its stop still cannot advance past it.
pub(crate) fn record(
    host: &dyn Host,
    path: &str,
    reason: Reason,
    question: Option<String>,
    summary: String,
) -> Result<Output, PluginError> {
    let step = next_of(host, path)?;
    let missing: Vec<String> = (step.gate.iter())
        .filter(|each| Check::blocking(each))
        .map(|each| each.check.clone())
        .collect();
    let at = format!("{path}/stops/{}", stamp(std::time::SystemTime::now()));
    let record = StopRecord {
        tag: Tag::default(),
        reason,
        phase: step.phase.clone(),
        missing: missing.clone(),
        question,
        summary,
    };
    board::put(
        host,
        &[json!({"path": at, "expect": "none", "record": record})],
    )?;
    Ok(Output {
        stop: at,
        phase: step.phase,
        missing,
    })
}

pub(crate) fn stop(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "stop", input)?;
    read(host, &given.path)?;
    if let Some(question) = &given.question {
        check_target(host, question, &[QUESTION_TYPE])?;
    }
    let written = record(
        host,
        &given.path,
        given.reason,
        given.question,
        given.summary,
    )?;
    Ok(to_value(written))
}

#[cfg(test)]
mod tests;

//! Whether a flow can run: every check of a kind Board knows, with the
//! parameters its kind needs and an id unique in the flow, a stop at one of
//! its phases, and only known placeholders in its text.
//!
//! `start` and `reflow` refuse a flow with a problem, and the `flow-valid`
//! check reads a drafted flow the same way and also needs a remedy on each
//! check that an owner or a reviewer passes, so an unattended agent knows
//! how to ask.

use std::collections::BTreeSet;
use std::fmt;

use board_ops::PluginError;
use board_plugin::shape::Open;
use serde_json::json;

use super::{Actor, Check, Flow, Kind};
use crate::gates::remedy::placeholders;
use crate::records::DONE;
use crate::refusal;

/// One thing wrong with a flow, and the phase and check it is in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) phase: Option<String>,
    pub(crate) check: Option<String>,
    pub(crate) what: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(phase) = &self.phase {
            write!(formatter, "phase {phase}, ")?;
        }
        if let Some(check) = &self.check {
            write!(formatter, "check {check}: ")?;
        }
        formatter.write_str(&self.what)
    }
}

/// The parameter a check of `kind` must name, or none when it needs none.
fn required(each: &Check, kind: Kind) -> Option<&'static str> {
    let (name, set) = match kind {
        Kind::Sections => ("sections", each.sections.is_some()),
        Kind::Field => ("field", each.field.is_some()),
        Kind::Records => ("type", each.record_type.is_some()),
        Kind::Understood => ("subject", each.subject.is_some()),
        Kind::ControlledPaths => ("controls", each.controls.is_some()),
        Kind::Exists => ("path", each.path.is_some()),
        _ => return None,
    };
    (!set).then_some(name)
}

/// The problems of `flow`, in its order. A `drafted` flow also needs a
/// remedy on each check whose actor is not the agent.
pub(crate) fn problems(flow: &Flow, drafted: bool) -> Vec<Problem> {
    let mut found = Vec::new();
    let names = flow.names();
    let stop = flow.stop.phase();
    if stop != DONE && !names.iter().any(|name| name == stop) {
        found.push(Problem {
            phase: None,
            check: None,
            what: format!(
                "the stop names the phase {stop}, which the flow lacks; name one of {} or done",
                names.join(", ")
            ),
        });
    }
    let mut ids = BTreeSet::new();
    for phase in &flow.phases {
        let at = |check: Option<&str>, what: String| Problem {
            phase: Some(phase.name.clone()),
            check: check.map(str::to_owned),
            what,
        };
        found.extend(unknown_placeholders(&phase.instruction).map(|word| {
            at(
                None,
                format!("the instruction names the unknown placeholder {{{word}}}"),
            )
        }));
        for each in &phase.checks {
            let id = Some(each.id.as_str());
            if !ids.insert(each.id.as_str()) {
                found.push(at(id, "another check of the flow has this id".to_owned()));
            }
            match &each.kind {
                Open::Other(kind) => found.push(at(
                    id,
                    format!("the kind {kind} is not one this Board knows"),
                )),
                Open::Known(kind) => {
                    if let Some(parameter) = required(each, *kind) {
                        found.push(at(
                            id,
                            format!("a check of kind {} needs {parameter}", kind.as_str()),
                        ));
                    }
                }
            }
            let texts =
                (each.remedy.iter().chain(each.path.iter())).chain(each.controls.iter().flatten());
            for word in texts.flat_map(|text| unknown_placeholders(text)) {
                found.push(at(
                    id,
                    format!("it names the unknown placeholder {{{word}}}"),
                ));
            }
            let asks = each.actor.is_some_and(|actor| actor != Actor::Agent);
            if drafted && asks && each.remedy.is_none() {
                found.push(at(
                    id,
                    "only the owner or a reviewer can pass it, and it has no remedy that tells \
                     an unattended agent how to ask"
                        .to_owned(),
                ));
            }
        }
    }
    found
}

/// The placeholders `text` names that Board does not fill.
fn unknown_placeholders(text: &str) -> impl Iterator<Item = String> + '_ {
    placeholders(text).filter(|word| !crate::gates::remedy::PLACEHOLDERS.contains(&word.as_str()))
}

/// `changes::flow::invalid` for the flow `flow`, which `by` chose, when it
/// has a problem.
pub(crate) fn refuse_invalid(flow: &Flow, by: &str) -> Result<(), PluginError> {
    let found = problems(flow, false);
    if found.is_empty() {
        return Ok(());
    }
    let said: Vec<String> = found.iter().map(ToString::to_string).collect();
    Err(refusal(
        "changes::flow::invalid",
        format!(
            "{by} names the flow {}, which cannot run: {}; fix the flow, or name another",
            flow.name,
            said.join("; ")
        ),
        [("flow", json!(flow.name)), ("problems", json!(said))],
    ))
}

/// `changes::mount::needs_repository` when `flow` has a check that reads a
/// repository's diff or check suite, which a change on the board itself,
/// `mount`, does not have.
pub(crate) fn refuse_without_repository(flow: &Flow, mount: &str) -> Result<(), PluginError> {
    let needing: Vec<&str> = (flow.phases.iter())
        .flat_map(|phase| &phase.checks)
        .filter(|each| {
            matches!(
                each.kind.known(),
                Some(Kind::Paths | Kind::Run | Kind::ControlledPaths)
            )
        })
        .map(|each| each.id.as_str())
        .collect();
    if needing.is_empty() {
        return Ok(());
    }
    Err(refusal(
        "changes::mount::needs_repository",
        format!(
            "the flow {} has checks that read a repository's diff or check suite ({}), and a \
             change on {mount}, the board itself, has no repository; name the repository's mount, \
             such as /repos/<name>, or a flow without those checks, such as flow-authoring",
            flow.name,
            needing.join(", ")
        ),
        [("flow", json!(flow.name)), ("checks", json!(needing))],
    ))
}

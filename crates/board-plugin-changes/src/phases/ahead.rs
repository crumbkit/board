//! `changes.next --through <phase>`: the checks of each phase after the
//! change's current one, up to and including `through`, so an agent sees
//! what a later gate needs before it gets there.
//!
//! A check is judged ahead only when its kind reads what the plan or the
//! change record already holds: `sections`, `considered`, `field`,
//! `exists`, `rule-paths` and `planned-rule-paths`. Every other kind needs
//! work that has not happened yet, so it is `later`.

use board_ops::{Host, PluginError};
use board_plugin::shape::Open;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::change::read;
use crate::flow::{self, Kind, of_change};
use crate::gates::{self, Observed, interpret, not_required, remedy};
use crate::records::ChangeRecord;
use crate::repo::check_suite;
use crate::{refusal, to_value};

use super::{NextOutput, next_of, observe_in};

/// One change, by path, and optionally the last phase to list the checks
/// of ahead of time.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NextInput {
    #[schemars(
        regex(pattern = "^/"),
        extend("format" = "board-path"),
        description = "The change's logical path, such as /changes/suite-timeout."
    )]
    pub(crate) path: String,
    #[schemars(
        length(min = 1),
        description = "A phase of the change's flow, such as land: `ahead` lists the checks of \
                       each phase after the current one, up to and including it."
    )]
    pub(crate) through: Option<String>,
}

/// What `next` says of the change, and, when `through` names a phase, the
/// checks of each phase after the current one up to it (`ahead`). Without
/// `through` there is no `ahead`, and the output is what `next` always
/// gave.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Next {
    #[serde(flatten)]
    next: NextOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead: Option<Vec<Ahead>>,
}

/// One later phase and each of its checks, in the flow's order.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Ahead {
    phase: String,
    checks: Vec<Judged>,
}

/// One check of a later phase: whether it refuses `advance` when missing
/// (`blocks`) and whether its `when` holds under the plan (`applies`). A
/// check that applies has a `status`: `pass` or `missing` when the plan or
/// the change record already settles it, or `later` when it needs work
/// that has not happened yet. `remedy` says what to do for a check that is
/// `missing` or `later`.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Judged {
    check: String,
    blocks: bool,
    applies: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remedy: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Pass,
    Missing,
    Later,
}

/// `next` for the change at `path`, with `ahead` when `through` names a
/// phase. A `through` that names no phase of the change's flow is refused
/// before the gate is read.
pub(crate) fn next(
    host: &dyn Host,
    path: &str,
    through: Option<&str>,
) -> Result<Value, PluginError> {
    let ahead = match through {
        Some(through) => Some(ahead(host, path, through)?),
        None => None,
    };
    Ok(to_value(Next {
        next: next_of(host, path)?,
        ahead,
    }))
}

/// The checks of each phase after the change's current one, up to and
/// including `through`.
fn ahead(host: &dyn Host, path: &str, through: &str) -> Result<Vec<Ahead>, PluginError> {
    let (change, _) = read(host, path)?;
    let Some(flow) = of_change(host, path, &change)? else {
        return Err(bad_through(path, through, &[]));
    };
    let names = flow.names();
    if flow.phase(through).is_none() {
        return Err(bad_through(path, through, &names));
    }
    let (Some(from), Some(to)) = (
        names.iter().position(|name| *name == change.phase),
        names.iter().position(|name| name == through),
    ) else {
        // A change outside its flow's phases, such as a done or paused
        // one, has no phase after its current one.
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for phase in flow.phases.iter().take(to + 1).skip(from + 1) {
        found.push(judge_phase(host, path, &change, &flow, phase)?);
    }
    Ok(found)
}

/// `changes::next::bad_through`, naming the flow's phases.
fn bad_through(path: &str, through: &str, phases: &[String]) -> PluginError {
    let message = if phases.is_empty() {
        format!(
            "the change at {path} follows no flow, so --through {through} names no phase of it; \
             run `board changes next --path {path}` without --through"
        )
    } else {
        format!(
            "--through {through} names no phase of the flow the change at {path} follows; name \
             one of {}",
            listed(phases)
        )
    };
    refusal(
        "changes::next::bad_through",
        message,
        [
            ("path", json!(path)),
            ("through", json!(through)),
            ("phases", json!(phases)),
        ],
    )
}

/// `names` as prose: `a`, `a and b`, or `a, b and c`.
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Whether a check of `kind` reads only what the plan or the change record
/// already holds, so it can be judged before its phase.
fn judged_ahead(kind: &Open<Kind>) -> bool {
    matches!(
        kind.known(),
        Some(
            Kind::Sections
                | Kind::Considered
                | Kind::Field
                | Kind::Exists
                | Kind::RulePaths
                | Kind::PlannedRulePaths
        )
    )
}

/// A check `observe` reads for in place of `each`, whose kind needs later
/// work: its `when` alone, so the gate reads what tells whether it
/// applies and nothing its kind would read.
fn when_only(each: &flow::Check) -> flow::Check {
    flow::Check {
        kind: Open::Known(Kind::Field),
        field: None,
        types: None,
        waivable: None,
        conditions: None,
        rules: None,
        path: None,
        ..each.clone()
    }
}

/// Each check of `phase`, judged as the gate would judge it once the
/// change reaches `phase`, where its kind allows.
fn judge_phase(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: &flow::Flow,
    phase: &flow::Phase,
) -> Result<Ahead, PluginError> {
    let judged: Vec<flow::Check> = (phase.checks.iter())
        .filter(|each| judged_ahead(&each.kind))
        .cloned()
        .collect();
    // The change as if it were in `phase`, under a flow whose `phase` holds
    // the checks judged ahead and the `when` of the rest, so `observe`
    // reads only what those need.
    let mut probe = flow.clone();
    if let Some(at) = probe.phases.iter_mut().find(|each| each.name == phase.name) {
        at.checks = (phase.checks.iter())
            .map(|each| {
                if judged_ahead(&each.kind) {
                    each.clone()
                } else {
                    when_only(each)
                }
            })
            .collect();
    }
    let mut there = change.clone();
    there.phase.clone_from(&phase.name);
    let mut observed = observe_in(host, path, &there, Some(probe))?;
    // A `later` check's remedy may name the suite, which the probe does not
    // read.
    if observed.suite.is_none() {
        observed.suite = if change.v1 {
            check_suite(host, &change.mount)?
        } else {
            change.suite.clone()
        };
    }
    let gate = interpret(&observed, &judged);
    let skipped = not_required(&observed, &phase.checks);
    let checks = (phase.checks.iter())
        .map(|each| judge(&observed, &gate, &skipped, each))
        .collect();
    Ok(Ahead {
        phase: phase.name.clone(),
        checks,
    })
}

/// `each` as `ahead` lists it: from `gate`, the checks judged ahead, or
/// `later`, or with no status when `skipped` names it.
fn judge(
    observed: &Observed,
    gate: &[gates::Check],
    skipped: &[String],
    each: &flow::Check,
) -> Judged {
    let blocks = each.blocks.unwrap_or(true);
    if skipped.contains(&each.id) {
        return Judged {
            check: each.id.clone(),
            blocks,
            applies: false,
            status: None,
            detail: None,
            remedy: None,
        };
    }
    if let Some(found) = gate.iter().find(|found| found.check == each.id) {
        return Judged {
            check: each.id.clone(),
            blocks,
            applies: true,
            status: Some(match found.status {
                gates::Status::Pass => Status::Pass,
                gates::Status::Missing => Status::Missing,
            }),
            detail: Some(found.detail.clone()).filter(|said| !said.is_empty()),
            remedy: found.remedy.clone(),
        };
    }
    Judged {
        check: each.id.clone(),
        blocks,
        applies: true,
        status: Some(Status::Later),
        detail: Some(format!(
            "it needs work that has not happened yet, so the gate judges it once the change is in \
             {}",
            observed.change.phase
        )),
        remedy: Some(remedy::of(each, &remedy::values(observed))),
    }
}

#[cfg(test)]
mod tests;

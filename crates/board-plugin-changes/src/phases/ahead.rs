//! `changes.next --through <phase>`: the checks of each phase after the
//! change's current one, up to and including `through`, so an agent sees
//! what a later gate needs before it gets there.
//!
//! A check is judged ahead only when its kind reads what the plan or the
//! change record already holds: `sections`, `considered`, `field`,
//! `exists`, `rule-paths` and `planned-rule-paths`. Every other kind needs
//! work that has not happened yet, so it is `later`.

use std::collections::BTreeMap;

use board_ops::{Host, PluginError};
use board_plugin::shape::Open;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::change::read;
use crate::flow::{self, Field, Flow, Kind, of_change};
use crate::gates::{self, Observed, applies, interpret, left_out, not_required, remedy, sections};
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
/// gave. `filled` says what Board recorded for the change before it read
/// the gate: the HEAD of the caller's checkout as the head of a change with
/// none, when the checkout is of the change's mount and on a branch other
/// than the one the mount follows, and `unowned` as the route of a change
/// with none; it is left out when Board recorded nothing.
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
/// before the fill writes anything or the gate is read. The later phases
/// are judged after the fill, so they see the head or route it recorded.
pub(crate) fn next(
    host: &dyn Host,
    path: &str,
    through: Option<&str>,
) -> Result<Value, PluginError> {
    if let Some(through) = through {
        flow_through(host, path, through)?;
    }
    let next = next_of(host, path)?;
    let ahead = match through {
        Some(through) => Some(ahead(host, path, through)?),
        None => None,
    };
    Ok(to_value(Next { next, ahead }))
}

/// The change at `path` and the flow it follows, when `through` names a
/// phase of that flow, and `changes::next::bad_through` otherwise.
fn flow_through(
    host: &dyn Host,
    path: &str,
    through: &str,
) -> Result<(ChangeRecord, Flow), PluginError> {
    let (change, _) = read(host, path)?;
    let Some(flow) = of_change(host, path, &change)? else {
        return Err(bad_through(path, through, &[]));
    };
    if flow.phase(through).is_none() {
        return Err(bad_through(path, through, &flow.names()));
    }
    Ok((change, flow))
}

/// The checks of each phase after the change's current one, up to and
/// including `through`, judged against the change as it is stored now.
fn ahead(host: &dyn Host, path: &str, through: &str) -> Result<Vec<Ahead>, PluginError> {
    let (change, flow) = flow_through(host, path, through)?;
    let names = flow.names();
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
    let plan = sections(observed.plan.as_deref().unwrap_or_default());
    let checks = (phase.checks.iter())
        .map(|each| judge(&observed, &plan, &gate, &skipped, each))
        .collect();
    Ok(Ahead {
        phase: phase.name.clone(),
        checks,
    })
}

/// `each` as `ahead` lists it, in this order: with no status when
/// `skipped` names it; `missing` when its `when` reads an answer the plan
/// leaves out or a section pattern that cannot be read, as the gate in its
/// phase would say; `later` when it reads a head, a pull request or a diff
/// the change does not have yet; else from `gate`, the checks judged
/// ahead; and `later` for every other kind.
fn judge(
    observed: &Observed,
    plan: &BTreeMap<String, String>,
    gate: &[gates::Check],
    skipped: &[String],
    each: &flow::Check,
) -> Judged {
    let judged = |status: Option<Status>, detail: Option<String>, remedy| Judged {
        check: each.id.clone(),
        blocks: each.blocks.unwrap_or(true),
        applies: status.is_some(),
        status,
        detail: detail.filter(|said| !said.is_empty()),
        remedy,
    };
    if skipped.contains(&each.id) {
        return judged(None, None, None);
    }
    if !left_out(observed, each).is_empty() {
        // The gate reports a left-out answer before it reads the check's
        // kind, so the `when` alone gives what it would say.
        let found = interpret(observed, &[when_only(each)]);
        let found = found.first();
        return judged(
            Some(Status::Missing),
            found.map(|found| found.detail.clone()),
            found.and_then(|found| found.remedy.clone()),
        );
    }
    if let Err(detail) = applies(observed, plan, each) {
        let remedy = remedy::of(each, &placeholders(observed));
        return judged(Some(Status::Missing), Some(detail), Some(remedy));
    }
    let found = gate.iter().find(|found| found.check == each.id);
    if let Some(found) = found.filter(|_| settled(observed, each)) {
        let status = match found.status {
            gates::Status::Pass => Status::Pass,
            gates::Status::Missing => Status::Missing,
        };
        return judged(
            Some(status),
            Some(found.detail.clone()),
            found.remedy.clone(),
        );
    }
    let detail = format!(
        "it needs work that has not happened yet, so the gate judges it once the change is in {}",
        observed.change.phase
    );
    let remedy = remedy::of(each, &placeholders(observed));
    judged(Some(Status::Later), Some(detail), Some(remedy))
}

/// Whether what `each` reads is already settled: false for a `field`
/// check on the head or the pull requests while the change has none, and
/// for a `rule-paths` check, which reads the diff, while it has no head.
fn settled(observed: &Observed, each: &flow::Check) -> bool {
    let change = &observed.change;
    match each.kind.known() {
        Some(Kind::Field) => match each.field {
            Some(Field::Head) => change.head.is_some(),
            Some(Field::Pulls) => !change.pulls().is_empty(),
            _ => true,
        },
        Some(Kind::RulePaths) => change.head.is_some(),
        _ => true,
    }
}

/// The placeholders' values for `observed`, each one with no value yet
/// written as itself in angle brackets, such as `<head>`, so a remedy for
/// later work reads whole.
fn placeholders(observed: &Observed) -> remedy::Values {
    let mut values = remedy::values(observed);
    for (name, value) in &mut values {
        if value.is_none() {
            *value = Some(format!("<{name}>"));
        }
    }
    values
}

#[cfg(test)]
mod tests;

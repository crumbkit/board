//! `changes.approve`: ask the owner to approve the change's current plan
//! with Touch ID, once the owner's attached answer chose an option that
//! names the change. The owner-approval check counts that signed approval,
//! which only the owner's key can make, where an answer's `by` and
//! `written_by` are claims any writer can make.

use std::collections::BTreeSet;

use board_model::{Code, Refusal};
use board_ops::{DecideReport, Host, PluginError, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::board::{logical, refused};
use crate::change::{attached, kind_of, read, referenced};
use crate::gates::kinds::{ANSWER_TYPE, chose_change};
use crate::gates::{Approval, Observed, Read};
use crate::phases::PathInput;
use crate::records::ChangeRecord;
use crate::{PLUGIN, refusal, to_value};

/// The capability the owner approves.
const APPROVE: &str = "changes.approve";

/// Whether the owner has approved, refused or not yet answered.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Asked,
    Approved,
    Refused,
}

/// The change, the pending or answered effect that asks the owner, and the
/// owner's answer so far.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ApproveOutput")]
pub(crate) struct Output {
    path: String,
    plan: String,
    /// The effect's id, which `board answer <effect> approve` answers.
    effect: Option<String>,
    state: State,
}

/// What the owner approves: the change and the plan it names now.
fn shown(path: &str, plan: &str) -> Shown {
    let record = json!({"change": path, "plan": plan});
    Shown::Record(serde_json::to_vec(&record).expect("a record of two strings serializes"))
}

/// Asks the owner, through `Host::decide`, to approve the change's current
/// plan. Writes only the pending effect, and nothing when the owner has
/// answered. Refused with `changes::approve::not_chosen` unless an attached
/// answer chose an option that names the change.
pub(crate) fn approve(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PathInput { path } = PluginError::read_input(PLUGIN, "approve", input)?;
    let path = path.trim_end_matches('/').to_owned();
    let (change, _) = read(host, &path)?;
    let records = referenced(host, &attached(host, &path)?)?;
    let observed = Observed::new(&path, change, kind_of(host, &path)?, records);
    let plan = chosen(&observed)?;
    let at = logical(&path, "decide")?;
    let decided = host.decide(APPROVE, &at, Some(&shown(&path, &plan)));
    Ok(to_value(outcome(path, plan, decided)?))
}

/// The change's plan, once an attached answer chose an option that names
/// the change; otherwise the refusal.
fn chosen(observed: &Observed) -> Result<String, PluginError> {
    let path = observed.path.as_str();
    let Some(plan) = observed.change.plan.clone() else {
        return Err(refusal(
            "changes::approve::no_plan",
            format!("{path} names no plan, so there is no plan for the owner to approve"),
            [("path", json!(path))],
        ));
    };
    let answers = observed.records.of_type(ANSWER_TYPE);
    if !(answers.into_iter()).any(|(_, body)| chose_change(observed, body)) {
        return Err(refusal(
            "changes::approve::not_chosen",
            format!(
                "no attached {ANSWER_TYPE} chose an option, in its attached question, that names \
                 {path}; ask the owner with {path} in the approving option, and attach the \
                 question and the answer with `board changes attach --path {path} --ref <path>`"
            ),
            [("path", json!(path))],
        ));
    }
    Ok(plan)
}

/// What the decide answered: approved, or asked or refused with the
/// effect's id. Any other refusal of Board's is passed on.
fn outcome(
    path: String,
    plan: String,
    decided: Result<DecideReport, Box<Refusal>>,
) -> Result<Output, PluginError> {
    let (state, effect) = match decided {
        Ok(report) => (State::Approved, report.effect),
        Err(refusal) => {
            let state = match refusal.code.as_str() {
                code if code == Code::SettingsAsked.as_str() => State::Asked,
                code if code == Code::SettingsRefused.as_str() => State::Refused,
                _ => return Err(refused("decide", &refusal)),
            };
            let effect = refusal.fields.get("effect").and_then(Value::as_str);
            (state, effect.map(str::to_owned))
        }
    };
    Ok(Output {
        path,
        plan,
        effect,
        state,
    })
}

/// Reads into `found`, when a check `wanted` reads it, what
/// `Host::decided` finds of the owner's approval of the change's current
/// plan.
pub(crate) fn read_approval(host: &dyn Host, found: &mut Observed, wanted: &BTreeSet<Read>) {
    if wanted.contains(&Read::Approval) {
        found.approval = approval(host, &found.path, &found.change);
    }
}

/// What `Host::decided` finds of the owner's approval of the change's
/// current plan. No personal board means no approval.
fn approval(host: &dyn Host, path: &str, change: &ChangeRecord) -> Approval {
    let Some(plan) = &change.plan else {
        return Approval::None;
    };
    let Ok(at) = path.parse() else {
        return Approval::None;
    };
    match host.decided(APPROVE, &at, Some(&shown(path, plan))) {
        Ok(Some(_)) => Approval::Signed { plan: plan.clone() },
        Ok(None) => Approval::None,
        Err(refusal) if refusal.code.as_str() == Code::SettingsNoPersonal.as_str() => {
            Approval::None
        }
        Err(refusal) => Approval::Unread(format!("{} {}", refusal.code.as_str(), refusal.message)),
    }
}

#[cfg(test)]
mod tests;

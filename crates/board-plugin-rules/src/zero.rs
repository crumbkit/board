//! A zero rule's verdict: whether the latest full run of a clean revision of
//! its suite, which `runs.latest` gives, reports each named measure as 0.

use std::collections::BTreeMap;

use board_ops::Host;
use board_plugin::Target;
use board_plugin::shape::Prose;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::index::Control;
use crate::records::{Checker, Kind, ShowAt, Zero};

/// Seconds in a day, for `within_days`.
const DAY_S: u64 = 86_400;

/// A zero rule and what its suite's latest qualifying run reports.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Zeroed {
    rule: String,
    requirement: String,
    #[serde(flatten)]
    found: Found,
    kind: Kind,
    show: ShowAt,
    /// How to close an obligation; left out for a standing control.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    close: Option<String>,
    /// What left an obligation owed; left out when it names nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    owed_by: Option<String>,
}

impl Zeroed {
    /// Whether the rule's measures are all 0 in a recent enough run.
    pub(crate) fn passed(&self) -> bool {
        self.found.state == State::Zero
    }
}

/// Each zero rule among `applying`, judged against its suite's latest run.
pub(crate) fn zeroed(
    host: &dyn Host,
    applying: &[&String],
    controls: &BTreeMap<String, Control>,
) -> Vec<Zeroed> {
    (applying.iter())
        .filter_map(|logical| {
            let control = &controls[*logical];
            let Some(Checker::Zero(rule)) = &control.checker else {
                return None;
            };
            Some(Zeroed {
                rule: (*logical).clone(),
                requirement: control.requirement.clone(),
                found: found(host, rule),
                kind: control.kind(),
                show: control.show(),
                close: control.kind().close(logical),
                owed_by: control.owed(),
            })
        })
        .collect()
}

/// What a zero rule found in its suite's latest qualifying run: the run's
/// path and age, null when there is none, each named measure's value, null
/// when the run lacks it, and the state. `code` is the refusal of the
/// `runs.latest` call, given only when the state is `unreadable`.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Found {
    pub(crate) suite: String,
    pub(crate) run: Option<String>,
    pub(crate) age_s: Option<u64>,
    pub(crate) measures: Vec<Measured>,
    pub(crate) state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<String>,
}

/// One named measure and the value the run recorded, null when it recorded
/// none.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Measured {
    name: String,
    value: Option<Value>,
}

// A zero rule's state. No doc comment, which would describe the inlined
// enum in the output schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub(crate) enum State {
    Zero,
    NotZero,
    NoRun,
    Stale,
    MissingMeasure,
    Unreadable,
}

/// The verdict of `rule`, read through one `runs.latest` call. A refused
/// call, such as for a suite that is gone or a board without the runs
/// plugin, is `unreadable` with its code and fails only this rule. A
/// measure that is absent or not a number is `missing_measure`; a measure
/// other than 0 is `not_zero`; a run older than `within_days` is `stale`.
pub(crate) fn found(host: &dyn Host, rule: &Zero) -> Found {
    let target: Target = "runs.latest"
        .parse()
        .expect("runs.latest is <plugin>.<capability>");
    let mut found = Found {
        suite: rule.suite.clone(),
        run: None,
        age_s: None,
        measures: (rule.measures.iter())
            .map(|name| Measured {
                name: name.clone(),
                value: None,
            })
            .collect(),
        state: State::NoRun,
        code: None,
    };
    let latest = match host.call(&target, &json!({"suite": rule.suite})) {
        Ok(report) => report.output,
        Err(refusal) => {
            found.state = State::Unreadable;
            found.code = Some(refusal.code.as_str().to_owned());
            return found;
        }
    };
    let Some(run) = latest.get("run").filter(|run| !run.is_null()) else {
        return found;
    };
    found.run = run["path"].as_str().map(str::to_owned);
    found.age_s = run["age_s"].as_u64();
    for measured in &mut found.measures {
        measured.value = run["measures"].get(&measured.name).cloned();
    }
    let numbers: Vec<Option<f64>> = (found.measures.iter())
        .map(|measured| measured.value.as_ref().and_then(Value::as_f64))
        .collect();
    let too_old = rule
        .within_days
        .is_some_and(|days| found.age_s.is_none_or(|age| age > u64::from(days) * DAY_S));
    found.state = if numbers.iter().any(Option::is_none) {
        State::MissingMeasure
    } else if numbers.iter().flatten().any(|number| number.abs() > 0.0) {
        State::NotZero
    } else if too_old {
        State::Stale
    } else {
        State::Zero
    };
    found
}

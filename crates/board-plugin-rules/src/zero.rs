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

/// The code of a suite with a run `runs.latest` could not read, which may
/// be newer than the run it gives.
const RUN_UNREADABLE: &str = "rules::zero::run_unreadable";

/// A zero rule and what its suite's latest qualifying run reports: the
/// run's path and age, null when there is none, each named measure's value,
/// null when the run lacks it, and the state. `code` says why the state is
/// `unreadable`, and is left out otherwise.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Zeroed {
    rule: String,
    requirement: String,
    suite: String,
    run: Option<String>,
    age_s: Option<u64>,
    measures: Vec<Measured>,
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
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
        self.state == State::Zero
    }
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
            let (run, code) = match latest(host, &rule.suite) {
                Ok(run) => (run, None),
                Err(code) => (Value::Null, Some(code)),
            };
            let measures: Vec<Measured> = (rule.measures.iter())
                .map(|name| Measured {
                    name: name.clone(),
                    value: run["measures"].get(name).cloned(),
                })
                .collect();
            Some(Zeroed {
                rule: (*logical).clone(),
                requirement: control.requirement.clone(),
                suite: rule.suite.clone(),
                run: run["path"].as_str().map(str::to_owned),
                age_s: run["age_s"].as_u64(),
                state: state(rule, &run, &measures, code.is_some()),
                measures,
                code,
                kind: control.kind(),
                show: control.show(),
                close: control.kind().close(logical),
                owed_by: control.owed(),
            })
        })
        .collect()
}

/// The latest qualifying run of `suite`, null when it has none, through one
/// `runs.latest` call. A refused call, such as for a suite that is gone or a
/// board without the runs plugin, gives its code. A suite with a run
/// `runs.latest` could not read gives [`RUN_UNREADABLE`], since that run may
/// be newer than the one it gives.
fn latest(host: &dyn Host, suite: &str) -> Result<Value, String> {
    let target: Target = "runs.latest"
        .parse()
        .expect("runs.latest is <plugin>.<capability>");
    let latest = (host.call(&target, &json!({"suite": suite})))
        .map_err(|refusal| refusal.code.as_str().to_owned())?
        .output;
    if latest["unreadable"].as_u64().is_some_and(|count| count > 0) {
        return Err(RUN_UNREADABLE.to_owned());
    }
    Ok(latest["run"].clone())
}

/// The state of `rule` given its `run` and `measures`. A measure that is
/// absent or not a number is `missing_measure`; a measure other than 0 is
/// `not_zero`; a run older than `within_days` is `stale`.
fn state(rule: &Zero, run: &Value, measures: &[Measured], unreadable: bool) -> State {
    let numbers =
        || (measures.iter()).map(|measured| measured.value.as_ref().and_then(Value::as_f64));
    let too_old = (rule.within_days).is_some_and(|days| {
        run["age_s"]
            .as_u64()
            .is_none_or(|age| age > u64::from(days) * DAY_S)
    });
    if unreadable {
        State::Unreadable
    } else if run.is_null() {
        State::NoRun
    } else if numbers().any(|number| number.is_none()) {
        State::MissingMeasure
    } else if numbers().flatten().any(|number| number.abs() > 0.0) {
        State::NotZero
    } else if too_old {
        State::Stale
    } else {
        State::Zero
    }
}

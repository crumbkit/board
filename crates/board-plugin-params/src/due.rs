//! `due`: the values that need an agent to run their heuristic again.

use board_ops::{Host, PluginError};
use board_plugin::shape::Slug;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::stored::{CheckRun, Fallback, check_runs, params, usable};

/// `due` takes no input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DueInput")]
pub(crate) struct Input {}

/// The values due, oldest first, and how many heuristics were read.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DueOutput")]
pub(crate) struct Output {
    due: Vec<Due>,
    checked: usize,
}

/// A value due, by name, and why.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Due {
    #[schemars(with = "Slug")]
    name: String,
    reason: Reason,
}

// Why a value is due. No doc comment, which would describe the inlined
// enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
enum Reason {
    CheckFailed,
    Stale,
    OnDefault,
    OutOfBounds,
    WrongHeuristic,
}

/// Each heuristic whose value is on the default or unusable, whose latest
/// check since the value was derived measured above `fails_above`, or whose
/// value is older than `recheck_after` calendar days. A value with one reason
/// is listed once, the first of those that holds. A value with no record, or
/// with an unreadable `derived`, sorts first; then by `derived`, then by name.
pub(crate) fn due(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {} = PluginError::read_input("params", "due", input)?;
    let params = params(host)?;
    let runs = check_runs(host)?;
    // A clock jiff cannot hold reads as the latest time, so a value comes due
    // early and only an agent run is lost.
    let now = jiff::Timestamp::try_from(host.settings().now).unwrap_or(jiff::Timestamp::MAX);
    let mut due = Vec::new();
    for (name, heuristic) in &params.heuristics {
        let stored = params.values.get(name);
        let derived = stored.and_then(|stored| stored.derived.parse::<jiff::Timestamp>().ok());
        let reason = match usable(name, heuristic, stored) {
            Err(Fallback::Missing | Fallback::TooLittle) => Some(Reason::OnDefault),
            Err(Fallback::OutOfBounds { .. }) => Some(Reason::OutOfBounds),
            Err(Fallback::WrongHeuristic { .. }) => Some(Reason::WrongHeuristic),
            Ok(_) if failed(&runs, name, derived, heuristic.check.fails_above) => {
                Some(Reason::CheckFailed)
            }
            Ok(_) if stale(now, derived, heuristic.recheck_after) => Some(Reason::Stale),
            Ok(_) => None,
        };
        if let Some(reason) = reason {
            due.push((derived, name.clone(), reason));
        }
    }
    due.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let output = Output {
        checked: params.heuristics.len(),
        due: (due.into_iter())
            .map(|(_, name, reason)| Due { name, reason })
            .collect(),
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings and numbers"))
}

/// Whether the latest check of `name` since `derived` measured above
/// `fails_above`.
fn failed(
    runs: &[CheckRun],
    name: &str,
    derived: Option<jiff::Timestamp>,
    fails_above: f64,
) -> bool {
    latest(runs, name, derived).is_some_and(|run| run.check > fails_above)
}

/// Whether `derived` is more than `days` calendar days before `now`. A
/// `derived` that cannot be read is stale. These are calendar days until the
/// frecency change switches `recheck_after` to active days, which only makes
/// a value come due early.
pub(crate) fn stale(now: jiff::Timestamp, derived: Option<jiff::Timestamp>, days: u64) -> bool {
    let limit = i64::try_from(days.saturating_mul(86_400)).unwrap_or(i64::MAX);
    derived
        .is_none_or(|derived| now.duration_since(derived) > jiff::SignedDuration::from_secs(limit))
}

/// The latest check run of `/suites/params-<name>` that started at or after
/// `derived`, when there is one; a check that ran before the value was
/// derived checked an earlier value.
fn latest<'a>(
    runs: &'a [CheckRun],
    name: &str,
    derived: Option<jiff::Timestamp>,
) -> Option<&'a CheckRun> {
    let suite = format!("/suites/params-{name}");
    (runs.iter())
        .filter(|run| run.suite == suite && derived.is_none_or(|derived| run.started >= derived))
        .max_by_key(|run| run.started)
}

//! `due`: the values that need an agent to run their heuristic again.

use board_ops::{Host, PluginError};
use board_plugin::shape::Slug;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::Data;
use crate::stored::{CheckRun, check_runs, params};

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
    OnDefault,
}

/// Each heuristic whose value is on the default, or whose latest check
/// since the value was derived measured above `fails_above`. A value with
/// no record, or with an unreadable `derived`, sorts first; then by
/// `derived`, then by name.
pub(crate) fn due(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {} = PluginError::read_input("params", "due", input)?;
    let params = params(host)?;
    let runs = check_runs(host)?;
    let mut due = Vec::new();
    for (name, heuristic) in &params.heuristics {
        let stored = params.values.get(name);
        let derived = stored.and_then(|stored| stored.derived.parse::<jiff::Timestamp>().ok());
        let reason = match (stored, derived) {
            (None, _) => Some(Reason::OnDefault),
            (Some(stored), _) if stored.data == Data::TooLittle => Some(Reason::OnDefault),
            (Some(_), derived) => latest(&runs, name, derived)
                .filter(|run| run.check > heuristic.check.fails_above)
                .map(|_| Reason::CheckFailed),
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

//! `latest`: the latest full run of a clean revision of a suite, and what
//! it gave.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::SuitePath;
use crate::qualifying::qualifying;
use crate::records::{MeasureValue, Outcome};

/// The latest full run of a clean revision of `suite`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "LatestInput")]
pub(crate) struct Input {
    /// The suite's logical path, such as /suites/board-bench, here or in a mounted board.
    #[schemars(with = "SuitePath")]
    suite: String,
}

/// The latest qualifying run, or `null` when none qualifies.
///
/// `not_counted` is how many runs of the suite were left out because they
/// were targeted, dirty or did not say. `unreadable` is how many were left
/// out because Board marks them degraded for anything but a broken
/// reference, or this plugin cannot read them.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "LatestOutput")]
pub(crate) struct Output {
    run: Option<Latest>,
    not_counted: usize,
    unreadable: usize,
}

/// A run, by path, and what it gave. `outcome` is `fail` when a result
/// failed or erred, else `pass`.
///
/// `age_s` is the whole seconds since the run finished, or since it started
/// when it records no finish.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Latest {
    path: String,
    #[schemars(extend("format" = "date-time"))]
    started: String,
    #[schemars(extend("format" = "date-time"))]
    finished: Option<String>,
    outcome: Verdict,
    #[schemars(with = "BTreeMap<String, MeasureValue>")]
    measures: Map<String, Value>,
    failed: Vec<String>,
    age_s: u64,
}

// Whether a run passed. No doc comment, which would describe the inlined
// enum.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Verdict {
    Pass,
    Fail,
}

/// The newest qualifying run, its outcome, measures, failed cases and age,
/// or `null` when none qualifies.
pub(crate) fn latest(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { suite } = PluginError::read_input("runs", "latest", input)?;
    let counted = qualifying(host, &suite)?;
    let run = counted.runs.into_iter().next().map(|(path, run)| {
        // A case that failed in several trials is listed once.
        let mut listed = BTreeSet::new();
        let failed: Vec<String> = (run.results.iter())
            .filter(|result| result.outcome.is_some_and(Outcome::failed))
            .filter(|result| listed.insert(result.case.as_str()))
            .map(|result| result.case.clone())
            .collect();
        let ended = run.finished.as_ref().unwrap_or(&run.started);
        Latest {
            path,
            age_s: ended.seconds_before(host.settings().now),
            started: run.started.text,
            finished: run.finished.map(|finished| finished.text),
            outcome: if failed.is_empty() {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measures: run.measures.unwrap_or_default(),
            failed,
        }
    });
    let output = Output {
        run,
        not_counted: counted.not_counted,
        unreadable: counted.unreadable,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and maps"))
}

#[cfg(test)]
mod tests;

//! `trend`: one measure over the full runs of a clean revision of a suite,
//! newest first.

use board_ops::{Host, PluginError};
use board_plugin::shape::whole;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SuitePath;
use crate::qualifying::{Run, qualifying};

/// The most points `trend` returns, and its default `limit`. The cap keeps
/// the output bounded (STANDARDS.md 23).
const MOST: u16 = 1000;

/// One measure over the full runs of a clean revision of `suite`, newest first.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TrendInput")]
pub(crate) struct Input {
    /// The suite's logical path, such as /suites/board-bench, here or in a mounted board.
    #[schemars(with = "SuitePath")]
    suite: String,
    // The description names a measure as people write it, without backticks.
    #[schemars(
        description = "The measure's name, such as median_ms.",
        length(min = 1)
    )]
    measure: String,
    /// The case whose measure to read, such as get; the run's own measure when left out.
    #[schemars(length(min = 1))]
    case: Option<String>,
    /// The most points to return, 1 to 1000; 1000 when left out.
    #[serde(default = "most", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 1000))]
    limit: u16,
}

fn most() -> u16 {
    MOST
}

/// Points, newest first.
///
/// `total` counts every point and `truncated` says whether the limit left
/// some out. `without_measure` is how many qualifying runs give no number
/// for the measure. `not_counted` is how many runs were targeted, dirty or
/// did not say, and `unreadable` how many Board marks degraded or this
/// plugin cannot read.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TrendOutput")]
pub(crate) struct Output {
    points: Vec<Point>,
    total: usize,
    truncated: bool,
    without_measure: usize,
    not_counted: usize,
    unreadable: usize,
}

/// One run's value of the measure.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Point {
    path: String,
    #[schemars(extend("format" = "date-time"))]
    started: String,
    value: f64,
}

/// A point for each qualifying run that gives a number for the measure, at
/// most `limit`, newest first.
pub(crate) fn trend(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        suite,
        measure,
        case,
        limit,
    } = PluginError::read_input("runs", "trend", input)?;
    let counted = qualifying(host, &suite)?;
    let qualified = counted.runs.len();
    let mut points: Vec<Point> = (counted.runs.into_iter())
        .filter_map(|(path, run)| {
            // A flag is no point on a trend, nor is a label.
            let value = measured(&run, &measure, case.as_deref())?.as_f64()?;
            Some(Point {
                path,
                started: run.started.text,
                value,
            })
        })
        .collect();
    let (total, limit) = (points.len(), usize::from(limit));
    points.truncate(limit);
    let output = Output {
        points,
        total,
        truncated: total > limit,
        without_measure: qualified - total,
        not_counted: counted.not_counted,
        unreadable: counted.unreadable,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// The run's own value of `measure`, or the lowest trial's for `case`;
/// `None` when it gives none.
fn measured<'a>(run: &'a Run, measure: &str, case: Option<&str>) -> Option<&'a Value> {
    let Some(case) = case else {
        return run.measures.as_ref()?.get(measure);
    };
    // The first of the lowest trials, as Python's `min` picks it.
    let first = (run.results.iter())
        .filter(|result| result.case == case)
        .reduce(|lowest, result| {
            if result.trial() < lowest.trial() {
                result
            } else {
                lowest
            }
        })?;
    first.measures.as_ref()?.get(measure)
}

#[cfg(test)]
mod tests;

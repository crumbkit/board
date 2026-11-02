//! The runs of a suite that count: full runs of a clean revision, newest
//! first. `latest` and `trend` read only these.

use std::collections::BTreeMap;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError, QueryReport, literal};
use board_plugin::shape::{Record, TREE_CODES};
use serde::Deserialize;
use serde_json::{Map, Number, Value};

use crate::records::{Outcome, RunRecord, Scope, SuiteRecord};
use crate::time::Time;

/// The command a refusal suggests for finding a suite's path.
const LIST_SUITES: &str =
    "board query \"SELECT logical FROM records WHERE type = 'runs/suite/v1'\"";

/// The qualifying runs by path, newest first, and how many runs were left
/// out and why.
pub(crate) struct Qualifying {
    pub(crate) runs: Vec<(String, Run)>,
    pub(crate) not_counted: usize,
    pub(crate) unreadable: usize,
}

/// A run as the plugin reads it.
pub(crate) struct Run {
    pub(crate) started: Time,
    pub(crate) finished: Option<Time>,
    pub(crate) results: Vec<Stored>,
    pub(crate) measures: Option<Map<String, Value>>,
}

/// A run as a board's own, possibly looser, schema may admit it, read as
/// the Python plugin's pydantic model read it. Null stands for a field left
/// out, and an unknown field is ignored. The fields named with `_` are read
/// only to check their type.
///
/// Pydantic also coerced `"false"`, 0 and 1 to a flag, a string of digits
/// or a flag to a whole number, and a number to a time. A shipped schema
/// admits none of them, so only a board's own looser schema lets one
/// through; such a run is unreadable here. Matching pydantic's coercion
/// table would replace this.
#[derive(Deserialize)]
struct StoredRun {
    #[serde(rename = "suite")]
    _suite: String,
    #[serde(rename = "by")]
    _by: String,
    started: String,
    scope: Scope,
    results: Vec<Stored>,
    dirty: Option<bool>,
    finished: Option<String>,
    #[serde(rename = "versions")]
    _versions: Option<BTreeMap<String, String>>,
    host: Option<StoredHost>,
    measures: Option<Map<String, Value>>,
    #[serde(rename = "setup")]
    _setup: Option<String>,
    #[serde(rename = "summary")]
    _summary: Option<String>,
    written_by: Option<String>,
}

#[derive(Deserialize)]
struct StoredHost {
    #[serde(rename = "os")]
    _os: String,
    #[serde(rename = "arch")]
    _arch: String,
    logical_cpus: Number,
}

/// What one case gave in a run.
#[derive(Deserialize)]
pub(crate) struct Stored {
    pub(crate) case: String,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) measures: Option<Map<String, Value>>,
    note: Option<String>,
    trial: Option<Number>,
    #[serde(rename = "evidence")]
    _evidence: Option<Vec<String>>,
}

impl Stored {
    /// The trial's number, 0 when the result gives none, as the Python
    /// plugin ordered trials.
    pub(crate) fn trial(&self) -> f64 {
        self.trial.as_ref().and_then(Number::as_f64).unwrap_or(0.0)
    }
}

/// The runs in `<suite>/runs/` that are full and say they are clean, newest
/// first, a tie going to the last path.
///
/// A suite's runs are the records of type `runs/run/v1` under its `runs/`
/// directory, read by display path, so a mounted board's suite works the
/// same as one here. A run Board marks degraded for anything but a broken
/// reference, or one this plugin cannot read, is counted as unreadable.
pub(crate) fn qualifying(host: &dyn Host, suite: &str) -> Result<Qualifying, PluginError> {
    let found = rows(
        host,
        &format!(
            "SELECT logical FROM records WHERE type = {} AND logical = {}",
            literal(SuiteRecord::TYPE),
            literal(suite)
        ),
    )?;
    if found.rows.is_empty() {
        return Err(PluginError {
            code: "runs::suite::not_found".to_owned(),
            message: format!(
                "no runs/suite/v1 record is at {suite}; name a suite by its logical path, without \
                 .json, and list the suites with `{LIST_SUITES}`"
            ),
            fields: Map::from_iter([("suite".to_owned(), Value::from(suite))]),
            hint: None,
        });
    }
    let prefix = literal(&format!("{suite}/runs/"));
    let report = rows(
        host,
        &format!(
            "SELECT logical, code, body FROM records WHERE type = {} \
             AND substr(logical, 1, length({prefix})) = {prefix}",
            literal(RunRecord::TYPE)
        ),
    )?;
    let mut counted = Qualifying {
        runs: Vec::new(),
        not_counted: 0,
        unreadable: 0,
    };
    for row in report.rows {
        let [Value::String(path), code, Value::String(body)] = &row[..] else {
            counted.unreadable += 1;
            continue;
        };
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        let Some((run, clean)) = tree.then(|| readable(body)).flatten() else {
            counted.unreadable += 1;
            continue;
        };
        if clean {
            counted.runs.push((path.clone(), run));
        } else {
            counted.not_counted += 1;
        }
    }
    (counted.runs).sort_by(|(a_path, a), (b_path, b)| {
        (b.started.micros, b_path).cmp(&(a.started.micros, a_path))
    });
    Ok(counted)
}

/// The run `body` holds, and whether it is full and says it is clean, or
/// `None` when the Python plugin's model would have refused it.
fn readable(body: &str) -> Option<(Run, bool)> {
    let record = parse_json(body.as_bytes()).ok()?;
    let run = StoredRun::deserialize(&record).ok()?;
    let started = Time::parse(&run.started)?;
    let finished = match &run.finished {
        Some(finished) => Some(Time::parse(finished)?),
        None => None,
    };
    let whole = |number: &Number| {
        number
            .as_f64()
            .is_some_and(|n| n >= 1.0 && n.fract() == 0.0)
    };
    let hook = |name: &str| {
        !name.is_empty()
            && (name.bytes()).all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    };
    let written = (run.written_by.as_deref())
        .is_none_or(|by| by.split_once('/').is_some_and(|(a, b)| hook(a) && hook(b)));
    let host = (run.host.as_ref()).is_none_or(|host| whole(&host.logical_cpus));
    let measured = |measures: &Option<Map<String, Value>>| {
        (measures.iter().flat_map(Map::values))
            .all(|value| matches!(value, Value::Number(_) | Value::String(_) | Value::Bool(_)))
    };
    let results = (run.results.iter()).all(|result| {
        let noted = !result.outcome.is_some_and(|o| o != Outcome::Pass) || result.note.is_some();
        (result.outcome.is_some() || result.measures.is_some())
            && noted
            && result.trial.as_ref().is_none_or(whole)
            && measured(&result.measures)
    });
    if !(written && host && results && measured(&run.measures)) {
        return None;
    }
    let clean = run.scope == Scope::Full && run.dirty == Some(false);
    let run = Run {
        started,
        finished,
        results: run.results,
        measures: run.measures,
    };
    Some((run, clean))
}

/// The rows of `sql`, every one of them.
fn rows(host: &dyn Host, sql: &str) -> Result<QueryReport, PluginError> {
    (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("runs", "query", &error.refusal()))
}

#[cfg(test)]
pub(crate) mod tests;

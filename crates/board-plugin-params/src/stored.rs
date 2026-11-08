//! The heuristics, values and check runs in this board, read through one
//! query each, and the bounds a value must keep.

use std::collections::BTreeMap;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::TREE_CODES;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::records::{Bounds, Data};

/// Every heuristic and value in this board's own `/params/`, by path.
const PARAMS: &str = "SELECT type, logical, code, body FROM records \
     WHERE type IN ('params/heuristic/v1', 'params/value/v1') \
     AND logical LIKE '/params/%' ORDER BY logical";

/// Every run of a `params-<name>` suite in this board's own `/suites/`.
const RUNS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'runs/run/v1' AND logical LIKE '/suites/params-%' ORDER BY logical";

/// The directory that holds the heuristics.
pub(crate) const HEURISTICS: &str = "/params/heuristics/";
/// The directory that holds the values.
pub(crate) const VALUES: &str = "/params/values/";

/// A heuristic as far as the plugin reads it; any other field is ignored.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Heuristic {
    pub(crate) default: f64,
    pub(crate) check: Check,
}

/// A heuristic's check as far as the plugin reads it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Check {
    pub(crate) fails_above: f64,
}

/// A value record as far as the plugin reads it; any other field is
/// ignored.
#[derive(Clone, Debug, Deserialize)]
pub struct Stored {
    pub heuristic: String,
    pub value: f64,
    pub scopes: Option<BTreeMap<String, f64>>,
    pub data: Data,
    pub derived: String,
}

/// The heuristics and the values, each by name.
#[derive(Debug, Default)]
pub(crate) struct Params {
    pub(crate) heuristics: BTreeMap<String, Heuristic>,
    pub(crate) values: BTreeMap<String, Stored>,
}

/// A record's body as JSON, unless Board marks it degraded for anything
/// but the tree.
fn body(code: &Value, body: &Value) -> Option<Value> {
    let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
    tree.then(|| parse_json(body.as_str()?.as_bytes()).ok())
        .flatten()
}

fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("params", "query", &error.refusal()))?;
    Ok(report.rows)
}

/// The readable heuristics and values in this board. A record Board marks
/// degraded for anything but the tree, or that the plugin cannot read, is
/// left out, so a reader treats it as absent.
pub(crate) fn params(host: &dyn Host) -> Result<Params, PluginError> {
    let mut params = Params::default();
    for row in rows(host, PARAMS)? {
        let [Value::String(kind), Value::String(path), code, text] = &row[..] else {
            continue;
        };
        let Some(record) = body(code, text) else {
            continue;
        };
        if let Some(name) = path.strip_prefix(HEURISTICS)
            && let Ok(heuristic) = Heuristic::deserialize(&record)
            && kind == "params/heuristic/v1"
        {
            params.heuristics.insert(name.to_owned(), heuristic);
        } else if let Some(name) = path.strip_prefix(VALUES)
            && let Ok(value) = Stored::deserialize(&record)
            && kind == "params/value/v1"
        {
            params.values.insert(name.to_owned(), value);
        }
    }
    Ok(params)
}

/// A check run: its suite, when it started, and the `check` it measured.
#[derive(Debug)]
pub(crate) struct CheckRun {
    pub(crate) suite: String,
    pub(crate) started: jiff::Timestamp,
    pub(crate) check: f64,
}

/// The readable runs of `params-<name>` suites that measured `check`, in
/// path order.
pub(crate) fn check_runs(host: &dyn Host) -> Result<Vec<CheckRun>, PluginError> {
    /// A run as far as the plugin reads it.
    #[derive(Deserialize)]
    struct Run {
        suite: String,
        started: String,
        measures: Option<Map<String, Value>>,
    }
    let mut found = Vec::new();
    for row in rows(host, RUNS)? {
        let [Value::String(_), code, text] = &row[..] else {
            continue;
        };
        let Some(run) = body(code, text).and_then(|record| Run::deserialize(&record).ok()) else {
            continue;
        };
        let check = (run.measures.as_ref())
            .and_then(|measures| measures.get("check"))
            .and_then(Value::as_f64);
        if let (Some(check), Ok(started)) = (check, run.started.parse()) {
            found.push(CheckRun {
                suite: run.suite,
                started,
                check,
            });
        }
    }
    Ok(found)
}

/// Why `put` would refuse the value `value` at `/params/values/<name>`
/// against the heuristic's `bounds`: `params::value::wrong_heuristic` when
/// its `heuristic` is not `/params/heuristics/<name>`, else
/// `params::value::out_of_bounds` when its `value` or a scope's value lies
/// outside them. `None` admits it.
pub fn refusal(name: &str, bounds: Bounds, value: &Stored) -> Option<PluginError> {
    let expected = format!("{HEURISTICS}{name}");
    if value.heuristic != expected {
        return Some(PluginError {
            code: "params::value::wrong_heuristic".to_owned(),
            message: format!(
                "the value {name} names the heuristic {}, but a value runs its own heuristic, {expected}",
                value.heuristic
            ),
            fields: fields(&[("name", json!(name)), ("heuristic", json!(value.heuristic))]),
            hint: Some(format!("set heuristic to {expected}")),
        });
    }
    let scoped = (value.scopes.iter().flatten()).map(|(scope, number)| (Some(scope), *number));
    let outside = std::iter::once((None, value.value))
        .chain(scoped)
        .find(|(_, number)| !(bounds.min..=bounds.max).contains(number))?;
    let (scope, number) = outside;
    let at = scope.map_or_else(String::new, |scope| format!(" for {scope}"));
    Some(PluginError {
        code: "params::value::out_of_bounds".to_owned(),
        message: format!(
            "the value {name} is {number}{at}, outside its heuristic's bounds {} to {}",
            bounds.min, bounds.max
        ),
        fields: fields(&[
            ("name", json!(name)),
            ("value", json!(number)),
            ("scope", json!(scope)),
            ("min", json!(bounds.min)),
            ("max", json!(bounds.max)),
        ]),
        hint: Some(format!(
            "derive a value from {} to {}, or ask the owner to widen the bounds in {expected}",
            bounds.min, bounds.max
        )),
    })
}

fn fields(pairs: &[(&str, Value)]) -> Map<String, Value> {
    (pairs.iter())
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

//! The heuristics, values and check runs in this board, read through one
//! query each, and the one test of whether a reader may use a value.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::TREE_CODES;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::SHIPPED;
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
    pub(crate) bounds: Bounds,
    pub(crate) check: Check,
    pub(crate) recheck_after: u64,
}

/// A heuristic's check as far as the plugin reads it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Check {
    pub(crate) fails_above: f64,
}

/// A value record as far as the plugin reads it; any other field is
/// ignored.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Stored {
    pub(crate) heuristic: String,
    pub(crate) value: f64,
    pub(crate) scopes: Option<BTreeMap<String, f64>>,
    pub(crate) data: Data,
    pub(crate) derived: String,
}

/// The heuristics and the values, each by name.
#[derive(Debug)]
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

/// Whether `name` is what the shared shape `Slug` admits; a test keeps the
/// two in step.
pub(crate) fn is_name(name: &str) -> bool {
    (name.split('-')).all(|word| {
        !word.is_empty()
            && (word.bytes()).all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// The heuristics the plugin ships, by name.
pub(crate) fn shipped() -> BTreeMap<String, Heuristic> {
    (SHIPPED.iter())
        .map(|(name, text)| {
            let heuristic = serde_json::from_str(text)
                .expect("a shipped heuristic is a record the plugin reads; a test reads each");
            ((*name).to_owned(), heuristic)
        })
        .collect()
}

/// The shipped heuristics, each overridden by a record of its name in this
/// board, and this board's other heuristics and its values. A record Board
/// marks degraded for anything but the tree, or that the plugin cannot read,
/// is left out, so a reader treats it as absent and a shipped heuristic of
/// that name still applies. A record whose path gives a name that is not a
/// `Slug` is left out too, because `params.due` could not name it.
pub(crate) fn params(host: &dyn Host) -> Result<Params, PluginError> {
    let mut params = Params {
        heuristics: shipped(),
        values: BTreeMap::new(),
    };
    for row in rows(host, PARAMS)? {
        let [Value::String(kind), Value::String(path), code, text] = &row[..] else {
            continue;
        };
        let Some(record) = body(code, text) else {
            continue;
        };
        if let Some(name) = path.strip_prefix(HEURISTICS).filter(|name| is_name(name))
            && let Ok(heuristic) = Heuristic::deserialize(&record)
            && kind == "params/heuristic/v1"
        {
            params.heuristics.insert(name.to_owned(), heuristic);
        } else if let Some(name) = path.strip_prefix(VALUES).filter(|name| is_name(name))
            && let Ok(value) = Stored::deserialize(&record)
            && kind == "params/value/v1"
        {
            params.values.insert(name.to_owned(), value);
        }
    }
    Ok(params)
}

/// The moment an RFC 3339 `text` names. humantime reads only `Z`, and a run's
/// `started` may hold any offset, so this removes an offset of the form
/// `+HH:MM` first and applies it. It reads nothing else a `date-time` allows
/// that humantime refuses, such as a lowercase `z`.
pub(crate) fn moment(text: &str) -> Option<SystemTime> {
    let cut = text.len().checked_sub(6)?;
    let [
        sign @ (b'+' | b'-'),
        hours @ ..,
        b':',
        minute_tens,
        minute_ones,
    ] = text.get(cut..).map_or(&[][..], str::as_bytes)
    else {
        return humantime::parse_rfc3339(text).ok();
    };
    let number = |digits: &[u8]| std::str::from_utf8(digits).ok()?.parse::<u64>().ok();
    let seconds = number(hours)? * 3_600 + number(&[*minute_tens, *minute_ones])? * 60;
    let utc = humantime::parse_rfc3339(&format!("{}Z", &text[..cut])).ok()?;
    let offset = Duration::from_secs(seconds);
    if *sign == b'+' {
        utc.checked_sub(offset)
    } else {
        utc.checked_add(offset)
    }
}

/// A check run: its suite, when it started, and the `check` it measured.
#[derive(Debug)]
pub(crate) struct CheckRun {
    pub(crate) suite: String,
    pub(crate) started: SystemTime,
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
        if let (Some(check), Some(started)) = (check, moment(&run.started)) {
            found.push(CheckRun {
                suite: run.suite,
                started,
                check,
            });
        }
    }
    Ok(found)
}

/// Why a reader uses a heuristic's default and not a value record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fallback {
    Missing,
    TooLittle,
    /// The first number in the record outside its heuristic's bounds, and
    /// its scope when it is not the record's `value`.
    OutOfBounds {
        number: f64,
        scope: Option<String>,
    },
    /// The heuristic the record names instead of its own.
    WrongHeuristic {
        named: String,
    },
}

/// The record a reader may use for the value `name`, or why it uses the
/// default. `board put` checks a record only against its own JSON Schema, so
/// a stored record can name another heuristic or hold a number outside the
/// bounds; both make the whole record unusable, even for a scope whose own
/// number is inside them, because an agent must derive the record again.
/// A record naming another heuristic is judged first. A record with too
/// little data is judged before the bounds, because readers use none of its
/// numbers.
pub(crate) fn usable<'a>(
    name: &str,
    heuristic: &Heuristic,
    stored: Option<&'a Stored>,
) -> Result<&'a Stored, Fallback> {
    let Some(stored) = stored else {
        return Err(Fallback::Missing);
    };
    if stored.heuristic != name {
        return Err(Fallback::WrongHeuristic {
            named: stored.heuristic.clone(),
        });
    }
    if stored.data == Data::TooLittle {
        return Err(Fallback::TooLittle);
    }
    let Bounds { min, max } = heuristic.bounds;
    let scoped = (stored.scopes.iter().flatten()).map(|(scope, number)| (Some(scope), *number));
    let outside = std::iter::once((None, stored.value))
        .chain(scoped)
        .find(|(_, number)| !(min..=max).contains(number));
    match outside {
        Some((scope, number)) => Err(Fallback::OutOfBounds {
            number,
            scope: scope.cloned(),
        }),
        None => Ok(stored),
    }
}

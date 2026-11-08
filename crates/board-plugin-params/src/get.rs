//! `get`: a calibrated value by name, for one scope, or the heuristic's
//! default and why it applies.

use board_ops::{Host, PluginError};
use board_plugin::shape::{Ref, Slug, Summary, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::ValueRecord;
use crate::stored::{Fallback, HEURISTICS, Heuristic, VALUES, params, usable};

/// The value `name` resolves to for `scope`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GetInput")]
pub(crate) struct Input {
    /// The value's name, such as overflow-agent-grace, which is also the name of its heuristic under /params/heuristics/ and of its value under /params/values/.
    #[schemars(with = "Slug")]
    name: String,
    /// The repository or project the value is for, by its path, such as /repos/board; the value's own `value` applies when the record lists no value for it or when this is left out.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    scope: Option<String>,
}

/// The number a value resolves to, and where it came from. `record` and
/// `derived` name the value record when `source` is `value`; `reason` says
/// why the default applies when `source` is `default`, and `warning` carries
/// the code and the words of a default a consumer should mention.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GetOutput")]
pub struct Got {
    #[schemars(with = "Slug")]
    pub name: String,
    pub value: f64,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<ValueRecord>>")]
    pub record: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Timestamp>")]
    pub derived: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<Warning>,
}

// Where a resolved value came from. No doc comment, which would describe
// the inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Source {
    Value,
    Default,
}

// Why the default applies. No doc comment, which would describe the
// inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Reason {
    Missing,
    TooLittle,
    OutOfBounds,
    WrongHeuristic,
}

/// A default applied because the value record is absent or unusable.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    pub code: WarningCode,
    #[schemars(with = "Summary")]
    pub message: String,
}

// The warning's stable code. No doc comment, which would describe the
// inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(inline)]
pub enum WarningCode {
    #[serde(rename = "params::value::missing")]
    Missing,
    #[serde(rename = "params::value::out_of_bounds")]
    OutOfBounds,
    #[serde(rename = "params::value::wrong_heuristic")]
    WrongHeuristic,
}

/// The value `name` resolves to for `scope` in the board `host` reads,
/// which a plugin built into Board calls in-process. With no heuristic of
/// that name, shipped or at `/params/heuristics/<name>`, it refuses with
/// `params::heuristic::not_found`, because there is no default to give, and
/// the caller falls back to its own built-in default and warns.
pub fn get(host: &dyn Host, name: &str, scope: Option<&str>) -> Result<Got, PluginError> {
    let params = params(host)?;
    let Some(heuristic) = params.heuristics.get(name) else {
        let path = format!("{HEURISTICS}{name}");
        let mut fields = Map::new();
        fields.insert("name".to_owned(), json!(name));
        return Err(PluginError {
            code: "params::heuristic::not_found".to_owned(),
            message: format!("no heuristic for the value {name} is shipped or at {path}"),
            fields,
            hint: Some(format!(
                "use your own built-in default and warn, or write the heuristic at {path}"
            )),
        });
    };
    Ok(match usable(name, heuristic, params.values.get(name)) {
        Err(fallback) => default(name, heuristic, &fallback),
        Ok(stored) => {
            let scoped = scope.and_then(|scope| stored.scopes.as_ref()?.get(scope));
            Got {
                name: name.to_owned(),
                value: scoped.copied().unwrap_or(stored.value),
                source: Source::Value,
                record: Some(format!("{VALUES}{name}")),
                derived: Some(stored.derived.clone()),
                reason: None,
                warning: None,
            }
        }
    })
}

/// The answer when `fallback` sends a reader to `heuristic`'s default.
fn default(name: &str, heuristic: &Heuristic, fallback: &Fallback) -> Got {
    let number = heuristic.default;
    let warning = |code, what: String| {
        Some(Warning {
            code,
            message: format!("{what}, so the default {number} applies"),
        })
    };
    let (reason, warning) = match fallback {
        Fallback::Missing => (
            Reason::Missing,
            warning(
                WarningCode::Missing,
                format!("no readable value record is at {VALUES}{name}"),
            ),
        ),
        Fallback::TooLittle => (Reason::TooLittle, None),
        Fallback::OutOfBounds {
            number: outside,
            scope,
        } => {
            let at = scope
                .as_ref()
                .map_or_else(String::new, |scope| format!(" for {scope}"));
            let (min, max) = (heuristic.bounds.min, heuristic.bounds.max);
            (
                Reason::OutOfBounds,
                warning(
                    WarningCode::OutOfBounds,
                    format!(
                        "the value {name} is {outside}{at}, outside its heuristic's bounds {min} to {max}"
                    ),
                ),
            )
        }
        Fallback::WrongHeuristic { named } => (
            Reason::WrongHeuristic,
            warning(
                WarningCode::WrongHeuristic,
                format!("the value {name} names the heuristic {named}, not {name}"),
            ),
        ),
    };
    Got {
        name: name.to_owned(),
        value: number,
        source: Source::Default,
        record: None,
        derived: None,
        reason: Some(reason),
        warning,
    }
}

pub(crate) fn call(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { name, scope } = PluginError::read_input("params", "get", input)?;
    let got = get(host, &name, scope.as_deref())?;
    Ok(serde_json::to_value(got).expect("Got holds only strings and numbers"))
}

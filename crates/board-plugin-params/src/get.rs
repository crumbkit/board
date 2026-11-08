//! `get`: a calibrated value by name, for one scope, or the heuristic's
//! default and why it applies.

use board_ops::{Host, PluginError};
use board_plugin::shape::{Ref, Slug, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{Data, ValueRecord};
use crate::stored::{HEURISTICS, VALUES, params};

/// The value `name` resolves to for `scope`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GetInput")]
pub(crate) struct Input {
    /// The value's name, such as overflow-agent-grace, which is also the name of its heuristic under /params/heuristics/ and of its value under /params/values/.
    #[schemars(with = "Slug")]
    name: String,
    /// The repository or project the value is for, by its path, such as /repos/board; the value's own `value` applies when the record lists no value for it or when this is left out.
    #[schemars(regex(pattern = r"^/\S*$"))]
    scope: Option<String>,
}

/// The number a value resolves to, and where it came from. `record` and
/// `derived` name the value record when `source` is `value`; `reason` says
/// why the default applies when `source` is `default`.
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
}

/// The code of the warning a consumer gives when `get` answers with
/// `reason` `missing`: no value record exists, so the default applies.
pub const MISSING: &str = "params::value::missing";

/// The value `name` resolves to for `scope` in the board `host` reads,
/// which a plugin built into Board calls in-process. With no heuristic at
/// `/params/heuristics/<name>`, it refuses with
/// `params::heuristic::not_found`, and the caller falls back to its own
/// built-in default.
pub fn get(host: &dyn Host, name: &str, scope: Option<&str>) -> Result<Got, PluginError> {
    let params = params(host)?;
    let Some(heuristic) = params.heuristics.get(name) else {
        let path = format!("{HEURISTICS}{name}");
        let mut fields = Map::new();
        fields.insert("name".to_owned(), json!(name));
        return Err(PluginError {
            code: "params::heuristic::not_found".to_owned(),
            message: format!("no heuristic for the value {name} is at {path}"),
            fields,
            hint: Some(format!(
                "use your own built-in default and warn, or run `board plugin enable params` \
                 and write the heuristic at {path}"
            )),
        });
    };
    let default = |reason| Got {
        name: name.to_owned(),
        value: heuristic.default,
        source: Source::Default,
        record: None,
        derived: None,
        reason: Some(reason),
    };
    Ok(match params.values.get(name) {
        None => default(Reason::Missing),
        Some(stored) if stored.data == Data::TooLittle => default(Reason::TooLittle),
        Some(stored) => {
            let scoped = scope.and_then(|scope| stored.scopes.as_ref()?.get(scope));
            Got {
                name: name.to_owned(),
                value: scoped.copied().unwrap_or(stored.value),
                source: Source::Value,
                record: Some(format!("{VALUES}{name}")),
                derived: Some(stored.derived.clone()),
                reason: None,
            }
        }
    })
}

pub(crate) fn call(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { name, scope } = PluginError::read_input("params", "get", input)?;
    let got = get(host, &name, scope.as_deref())?;
    Ok(serde_json::to_value(got).expect("Got holds only strings and numbers"))
}

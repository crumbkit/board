//! The documents Board and a plugin exchange (docs/plugins.md, Calling a
//! capability). Every one carries `$type` and refuses unknown fields.

use std::path::Path;

use board_model::{
    CapabilityName, JsonError, PluginCode, PluginCodeError, PluginName, canonical_json, parse_json,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

pub const CALL_TYPE: &str = "board/call/v1";
pub const CALL_RESULT_TYPE: &str = "board/call-result/v1";
pub const ERROR_TYPE: &str = "board/error/v1";
pub const EXTRACT_TYPE: &str = "board/extract/v1";
pub const EXTRACT_RESULT_TYPE: &str = "board/extract-result/v1";

/// The `board/call/v1` request's bytes.
pub(crate) fn request(
    capability: &CapabilityName,
    input: &Value,
    root: &Path,
    database: &Path,
) -> Vec<u8> {
    canonical_json(&json!({
        "$type": CALL_TYPE,
        "capability": capability.as_str(),
        "input": input,
        "board": {
            "root": root.to_string_lossy(),
            "database": database.to_string_lossy(),
        },
    }))
    .into_bytes()
}

/// A refusal the plugin reported in `board/error/v1`.
#[derive(Debug)]
pub struct PluginFailure {
    pub code: PluginCode,
    pub message: String,
    pub fields: Map<String, Value>,
}

/// One document a plugin wrote: the result asked for, or an error.
#[derive(Debug)]
pub(crate) enum Reply<T> {
    Result(T),
    Error(PluginFailure),
}

/// `board/call-result/v1`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCallResult {
    #[serde(rename = "$type")]
    _tag: String,
    pub(crate) output: Value,
}

/// `board/extract-result/v1`: the blobs the plugin handled.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawExtractResult {
    #[serde(rename = "$type")]
    _tag: String,
    pub(crate) blobs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawError {
    #[serde(rename = "$type")]
    _tag: String,
    code: String,
    message: String,
    #[serde(default)]
    fields: Map<String, Value>,
}

/// Reads the one document `plugin` wrote to stdout: a result of type
/// `expected`, read as `T`, or an error.
pub(crate) fn reply<T: DeserializeOwned>(
    plugin: &PluginName,
    stdout: &[u8],
    expected: &'static str,
) -> Result<Reply<T>, BadOutput> {
    let value = parse_json(stdout).map_err(|source| BadOutput::Json { source })?;
    let shape = |source| BadOutput::Shape { source };
    match value.get("$type").and_then(Value::as_str) {
        Some(found) if found == expected => {
            Ok(Reply::Result(T::deserialize(value).map_err(shape)?))
        }
        Some(ERROR_TYPE) => {
            let raw = RawError::deserialize(value).map_err(shape)?;
            Ok(Reply::Error(PluginFailure {
                code: PluginCode::parse(plugin, raw.code)
                    .map_err(|PluginCodeError { code }| BadOutput::Code { code })?,
                message: raw.message,
                fields: raw.fields,
            }))
        }
        found => Err(BadOutput::Type {
            found: found.map(str::to_owned),
            expected,
        }),
    }
}

/// Why a plugin's stdout is not an answer Board accepts.
#[derive(Debug, thiserror::Error)]
pub enum BadOutput {
    #[error("its stdout is not one strict JSON document: {source}")]
    Json { source: JsonError },
    #[error("its document has the wrong fields: {source}")]
    Shape { source: serde_json::Error },
    #[error("its document has $type {found:?}, not {expected} or {ERROR_TYPE}")]
    Type {
        found: Option<String>,
        expected: &'static str,
    },
    #[error("its error code {code:?} is not <plugin>::<area>::<reason> with its own name")]
    Code { code: String },
    #[error("it exited {exit} with {written}")]
    Exit { exit: i32, written: &'static str },
    #[error("its output {source}")]
    Output { source: board_validate::RecordError },
    #[error("it names blob {blob:?}, which it was not sent")]
    Unsent { blob: String },
}

#[cfg(test)]
mod tests;

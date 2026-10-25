//! `plugin.json`, a `board/plugin-manifest/v1` document (docs/plugins.md, The
//! manifest).

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub use board_model::MANIFEST_TYPE;
use board_model::{Code, JsonError, RecordType, parse_json};
use board_validate::{Schema, SchemaError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Lowercase ASCII letters, digits and hyphens, at least one.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

macro_rules! name {
    ($(#[$doc:meta])* $type:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $type(String);

        impl $type {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $type {
            type Error = NameError;

            fn try_from(text: String) -> Result<Self, NameError> {
                if is_name(&text) {
                    Ok(Self(text))
                } else {
                    Err(NameError { text })
                }
            }
        }

        impl FromStr for $type {
            type Err = NameError;

            fn from_str(text: &str) -> Result<Self, NameError> {
                Self::try_from(text.to_owned())
            }
        }

        impl From<$type> for String {
            fn from(name: $type) -> Self {
                name.0
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

name!(
    /// A plugin's name, which is also its directory's name.
    PluginName
);
name!(
    /// A capability's name, unique within its plugin.
    CapabilityName
);

/// Text that is not a plugin or capability name.
#[derive(Debug, thiserror::Error)]
#[error("{text:?} is not a name: use lowercase ASCII letters, digits and hyphens")]
pub struct NameError {
    text: String,
}

/// The variables Board sets for every plugin. A capability's `env` may name
/// none of them.
pub(crate) const PASSED: [&str; 12] = [
    "PATH",
    "HOME",
    "LANG",
    "TZ",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "BOARD_TIMINGS",
    "BOARD_CALLER_DIR",
    "BOARD_DAY_START",
    "BOARD_CALL_DEPTH",
    "BOARD_PLUGIN",
];

/// An environment variable's name: ASCII letters, digits and underscores,
/// not starting with a digit, and none Board sets for every plugin.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct VariableName(String);

impl VariableName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for VariableName {
    type Error = VariableNameError;

    fn try_from(text: String) -> Result<Self, VariableNameError> {
        let mut bytes = text.bytes();
        let first = bytes.next();
        if !(first.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
        {
            Err(VariableNameError::Shape { text })
        } else if PASSED.contains(&text.as_str()) {
            Err(VariableNameError::Board { text })
        } else {
            Ok(Self(text))
        }
    }
}

impl FromStr for VariableName {
    type Err = VariableNameError;

    fn from_str(text: &str) -> Result<Self, VariableNameError> {
        Self::try_from(text.to_owned())
    }
}

impl From<VariableName> for String {
    fn from(name: VariableName) -> Self {
        name.0
    }
}

/// The longest `timeout` a capability may set, in seconds: an hour.
const MAX_TIMEOUT: u32 = 3600;

/// A capability's time limit: whole seconds from 1 to [`MAX_TIMEOUT`].
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "u32")]
struct Timeout(u32);

impl TryFrom<u32> for Timeout {
    type Error = String;

    fn try_from(seconds: u32) -> Result<Self, String> {
        if (1..=MAX_TIMEOUT).contains(&seconds) {
            Ok(Self(seconds))
        } else {
            Err(format!(
                "a timeout of {seconds} seconds is outside 1 to {MAX_TIMEOUT}; set one in that range, \
                 or split the capability's work into calls that each finish within an hour"
            ))
        }
    }
}

/// Text that is not an environment variable's name, or names one Board sets.
#[derive(Debug, thiserror::Error)]
pub enum VariableNameError {
    #[error(
        "{text:?} is not a variable name: use ASCII letters, digits and underscores, not starting with a digit"
    )]
    Shape { text: String },
    #[error(
        "{text:?} is a variable Board sets for every plugin; remove it from env, since the plugin \
         already sees Board's value"
    )]
    Board { text: String },
}

/// `MAJOR.MINOR.PATCH`, each part ASCII digits.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version(String);

impl Version {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Version {
    type Error = VersionError;

    fn try_from(text: String) -> Result<Self, VersionError> {
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            Ok(Self(text))
        } else {
            Err(VersionError { text })
        }
    }
}

impl From<Version> for String {
    fn from(version: Version) -> Self {
        version.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Text that is not a version.
#[derive(Debug, thiserror::Error)]
#[error("{text:?} is not a version: use MAJOR.MINOR.PATCH in digits")]
pub struct VersionError {
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(rename = "$type")]
    tag: String,
    name: PluginName,
    version: Version,
    description: String,
    run: Vec<String>,
    #[serde(default)]
    metadata: Map<String, Value>,
    #[serde(default)]
    extract: Vec<RecordType>,
    #[serde(default)]
    types: Vec<RecordType>,
    #[serde(default)]
    capabilities: Vec<RawCapability>,
}

/// An MCP Tool object (MCP 2025-11-25), with `description` and both schemas
/// required, and `icons` and `execution` refused.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawCapability {
    name: CapabilityName,
    #[serde(default)]
    title: Option<String>,
    description: String,
    input_schema: Value,
    output_schema: Value,
    #[serde(default)]
    annotations: Option<Annotations>,
    #[serde(default, rename = "_meta")]
    meta: Map<String, Value>,
    #[serde(default)]
    timeout: Option<Timeout>,
    #[serde(default)]
    env: Vec<VariableName>,
}

/// MCP's `ToolAnnotations`: hints Board stores and acts on none of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Annotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// A plugin's manifest, its schemas compiled.
#[derive(Debug)]
pub struct Manifest {
    pub name: PluginName,
    pub version: Version,
    pub description: String,
    /// The program and its arguments; never empty.
    pub run: Vec<String>,
    pub metadata: Map<String, Value>,
    pub extract: Vec<RecordType>,
    /// The types whose schemas the plugin ships, each in its own namespace.
    pub types: Vec<RecordType>,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug)]
pub struct Capability {
    pub name: CapabilityName,
    pub title: Option<String>,
    pub description: String,
    pub input: Schema,
    pub output: Schema,
    pub annotations: Option<Annotations>,
    pub meta: Map<String, Value>,
    /// How long a call may run, in place of Board's default limit.
    pub timeout: Option<Duration>,
    /// The variables of Board's environment that a call also receives.
    pub env: Vec<VariableName>,
}

impl Manifest {
    /// Parses the manifest in the directory named `directory`.
    pub fn parse(directory: &str, bytes: &[u8]) -> Result<Self, ManifestError> {
        let value = parse_json(bytes).map_err(|source| ManifestError::Json { source })?;
        let manifest = Self::from_value(value)?;
        if manifest.name.as_str() != directory {
            return Err(ManifestError::Directory {
                name: manifest.name,
                directory: directory.to_owned(),
            });
        }
        Ok(manifest)
    }

    /// Reads a parsed manifest by its content alone, wherever it is stored.
    pub fn from_value(value: Value) -> Result<Self, ManifestError> {
        let raw =
            RawManifest::deserialize(value).map_err(|source| ManifestError::Shape { source })?;
        if raw.tag != MANIFEST_TYPE {
            return Err(ManifestError::Type { found: raw.tag });
        }
        if raw.run.is_empty() {
            return Err(ManifestError::EmptyRun);
        }
        let own = format!("{}/", raw.name);
        for (at, record_type) in raw.types.iter().enumerate() {
            if record_type.is_board() {
                return Err(ManifestError::BoardType {
                    record_type: record_type.clone(),
                });
            }
            if !record_type.as_str().starts_with(&own) {
                return Err(ManifestError::ForeignType {
                    record_type: record_type.clone(),
                });
            }
            if raw.types[..at].contains(record_type) {
                return Err(ManifestError::DuplicateType {
                    record_type: record_type.clone(),
                });
            }
        }
        let mut capabilities: Vec<Capability> = Vec::with_capacity(raw.capabilities.len());
        for capability in raw.capabilities {
            if capabilities.iter().any(|seen| seen.name == capability.name) {
                return Err(ManifestError::DuplicateCapability {
                    name: capability.name,
                });
            }
            let compile = |document: Value, side| {
                if document.get("type").and_then(Value::as_str) != Some("object") {
                    return Err(ManifestError::NotObject {
                        capability: capability.name.clone(),
                        side,
                    });
                }
                Schema::compile(document).map_err(|source| ManifestError::Schema {
                    capability: capability.name.clone(),
                    side,
                    source,
                })
            };
            capabilities.push(Capability {
                input: compile(capability.input_schema, Side::Input)?,
                output: compile(capability.output_schema, Side::Output)?,
                name: capability.name,
                title: capability.title,
                description: capability.description,
                annotations: capability.annotations,
                meta: capability.meta,
                timeout: capability
                    .timeout
                    .map(|Timeout(seconds)| Duration::from_secs(seconds.into())),
                env: capability.env,
            });
        }
        Ok(Self {
            name: raw.name,
            version: raw.version,
            description: raw.description,
            run: raw.run,
            metadata: raw.metadata,
            extract: raw.extract,
            types: raw.types,
            capabilities,
        })
    }

    /// Keeps only the capabilities named `capability`, when given, that
    /// match every filter.
    pub fn retain(&mut self, capability: Option<&CapabilityName>, filters: &[MetaFilter]) {
        let plugin = &self.metadata;
        self.capabilities.retain(|offered| {
            capability.is_none_or(|name| *name == offered.name)
                && filters
                    .iter()
                    .all(|filter| filter.matches(&offered.meta, plugin))
        });
    }
}

/// Which of a capability's schemas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Input,
    Output,
}

impl fmt::Display for Side {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "inputSchema",
            Self::Output => "outputSchema",
        })
    }
}

/// `<key>=<value>`: metadata holding `key` whose value is the string
/// `value`. A capability's `_meta` answers for a key it holds; the plugin's
/// `metadata` answers for the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaFilter {
    key: String,
    value: String,
}

impl MetaFilter {
    fn matches(&self, capability: &Map<String, Value>, plugin: &Map<String, Value>) -> bool {
        capability
            .get(&self.key)
            .or_else(|| plugin.get(&self.key))
            .and_then(Value::as_str)
            == Some(self.value.as_str())
    }
}

impl FromStr for MetaFilter {
    type Err = MetaFilterError;

    fn from_str(text: &str) -> Result<Self, MetaFilterError> {
        match text.split_once('=') {
            Some((key, value)) if !key.is_empty() => Ok(Self {
                key: key.to_owned(),
                value: value.to_owned(),
            }),
            _ => Err(MetaFilterError {
                text: text.to_owned(),
            }),
        }
    }
}

/// Text that is not `<key>=<value>`.
#[derive(Debug, thiserror::Error)]
#[error("{text:?} is not <key>=<value> with a non-empty key")]
pub struct MetaFilterError {
    text: String,
}

/// A manifest Board refuses.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("the plugin directory holds no readable plugin.json")]
    Missing,
    #[error("the manifest is not strict JSON: {source}")]
    Json { source: JsonError },
    #[error("the manifest is not a plugin manifest: {source}")]
    Shape { source: serde_json::Error },
    #[error("the manifest has $type {found:?}, not {MANIFEST_TYPE}")]
    Type { found: String },
    #[error("plugin.json names {name}, but its directory is {directory:?}")]
    Directory { name: PluginName, directory: String },
    #[error("the manifest has an empty run")]
    EmptyRun,
    #[error("the manifest ships type {record_type}, which is outside the plugin's namespace")]
    ForeignType { record_type: RecordType },
    #[error("the manifest ships type {record_type}, but the `board` namespace is Board's own")]
    BoardType { record_type: RecordType },
    #[error("the manifest ships type {record_type} twice")]
    DuplicateType { record_type: RecordType },
    #[error("the manifest offers capability {name} twice")]
    DuplicateCapability { name: CapabilityName },
    #[error("the manifest capability {capability} has an {side} whose root is not an object type")]
    NotObject {
        capability: CapabilityName,
        side: Side,
    },
    #[error("the manifest capability {capability} has an {side} that cannot be used: {source}")]
    Schema {
        capability: CapabilityName,
        side: Side,
        source: SchemaError,
    },
    #[error("the schema the plugin ships for {record_type} at {} is missing or cannot be read", path.display())]
    ShippedMissing {
        record_type: RecordType,
        path: PathBuf,
    },
    #[error("the schema the plugin ships for {record_type} at {} is not strict JSON: {source}", path.display())]
    ShippedJson {
        record_type: RecordType,
        path: PathBuf,
        source: JsonError,
    },
    #[error("the schema the plugin ships for {record_type} at {} does not compile: {source}", path.display())]
    ShippedSchema {
        record_type: RecordType,
        path: PathBuf,
        source: SchemaError,
    },
}

impl ManifestError {
    pub fn code(&self) -> Code {
        match self {
            Self::Missing
            | Self::Json { .. }
            | Self::Shape { .. }
            | Self::Type { .. }
            | Self::Directory { .. }
            | Self::EmptyRun
            | Self::ForeignType { .. }
            | Self::BoardType { .. }
            | Self::DuplicateType { .. }
            | Self::DuplicateCapability { .. }
            | Self::NotObject { .. }
            | Self::Schema { .. }
            | Self::ShippedMissing { .. }
            | Self::ShippedJson { .. }
            | Self::ShippedSchema { .. } => Code::PluginBadManifest,
        }
    }
}

#[cfg(test)]
mod tests;

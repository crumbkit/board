//! `plugin.json`, a `board/plugin-manifest/v1` document (docs/plugins.md, The
//! manifest).

use std::fmt;
use std::str::FromStr;

use board_model::{Code, JsonError, RecordType, parse_json};
use board_validate::{Schema, SchemaError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const MANIFEST_TYPE: &str = "board/plugin-manifest/v1";

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
    capabilities: Vec<RawCapability>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapability {
    name: CapabilityName,
    description: String,
    input_schema: Value,
    output_schema: Value,
    #[serde(default)]
    metadata: Map<String, Value>,
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
    pub capabilities: Vec<Capability>,
}

#[derive(Debug)]
pub struct Capability {
    pub name: CapabilityName,
    pub description: String,
    pub input: Schema,
    pub output: Schema,
    pub metadata: Map<String, Value>,
}

impl Manifest {
    /// Parses the manifest in the directory named `directory`.
    pub fn parse(directory: &str, bytes: &[u8]) -> Result<Self, ManifestError> {
        let value = parse_json(bytes).map_err(|source| ManifestError::Json { source })?;
        let raw =
            RawManifest::deserialize(value).map_err(|source| ManifestError::Shape { source })?;
        if raw.tag != MANIFEST_TYPE {
            return Err(ManifestError::Type { found: raw.tag });
        }
        if raw.name.as_str() != directory {
            return Err(ManifestError::Directory {
                name: raw.name,
                directory: directory.to_owned(),
            });
        }
        if raw.run.is_empty() {
            return Err(ManifestError::EmptyRun);
        }
        let mut capabilities: Vec<Capability> = Vec::with_capacity(raw.capabilities.len());
        for capability in raw.capabilities {
            if capabilities.iter().any(|seen| seen.name == capability.name) {
                return Err(ManifestError::DuplicateCapability {
                    name: capability.name,
                });
            }
            let compile = |document, side| {
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
                description: capability.description,
                metadata: capability.metadata,
            });
        }
        Ok(Self {
            name: raw.name,
            version: raw.version,
            description: raw.description,
            run: raw.run,
            metadata: raw.metadata,
            extract: raw.extract,
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
                    .all(|filter| filter.matches(&offered.metadata, plugin))
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
            Self::Input => "input_schema",
            Self::Output => "output_schema",
        })
    }
}

/// `<key>=<value>`: metadata holding `key` whose value is the string
/// `value`. A capability's metadata answers for a key it holds; the plugin's
/// answers for the rest.
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
    #[error("plugin.json is not strict JSON: {source}")]
    Json { source: JsonError },
    #[error("plugin.json is not a plugin manifest: {source}")]
    Shape { source: serde_json::Error },
    #[error("plugin.json has $type {found:?}, not {MANIFEST_TYPE}")]
    Type { found: String },
    #[error("plugin.json names {name}, but its directory is {directory:?}")]
    Directory { name: PluginName, directory: String },
    #[error("plugin.json has an empty run")]
    EmptyRun,
    #[error("plugin.json offers capability {name} twice")]
    DuplicateCapability { name: CapabilityName },
    #[error("plugin.json capability {capability} has an {side} that cannot be used: {source}")]
    Schema {
        capability: CapabilityName,
        side: Side,
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
            | Self::DuplicateCapability { .. }
            | Self::Schema { .. } => Code::PluginBadManifest,
        }
    }
}

#[cfg(test)]
mod tests;

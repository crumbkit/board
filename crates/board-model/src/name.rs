//! Plugin and capability names, and the codes a plugin refuses with.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

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

/// A plugin's error code, `<plugin>::<area>::<reason>`, where area and
/// reason are lowercase ASCII letters, digits and underscores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCode(String);

impl PluginCode {
    /// `text` as a code of `plugin`'s own.
    pub fn parse(plugin: &PluginName, text: String) -> Result<Self, PluginCodeError> {
        let parts: Vec<&str> = text.split("::").collect();
        let snake = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        };
        match parts[..] {
            [owner, area, reason] if owner == plugin.as_str() && snake(area) && snake(reason) => {
                Ok(Self(text))
            }
            _ => Err(PluginCodeError { code: text }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Text that is not a code of the plugin that gave it.
#[derive(Debug, thiserror::Error)]
#[error("{code:?} is not <plugin>::<area>::<reason> with its own name")]
pub struct PluginCodeError {
    pub code: String,
}

#[cfg(test)]
mod tests;

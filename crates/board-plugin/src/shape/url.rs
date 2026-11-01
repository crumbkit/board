//! A web address in a record: an absolute `http` or `https` URI with a host
//! (docs/architecture/records/shapes.md, rule 6).

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use fluent_uri::Uri;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// The schema's pattern. `format: uri` admits `https://` and `https://:80`,
/// which have no host, so the pattern asks for a first host character after
/// any user information. JSON Schema patterns are case-sensitive, as the
/// scheme check is.
const PATTERN: &str = "^https?://([^/?#@]*@)?[^/?#@:][^/?#@]*([/?#]|$)";

/// An absolute `http` or `https` URI whose authority has a host that is not
/// empty. Its schema admits exactly the strings [`Url::from_str`] admits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Url(String);

impl Url {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Url {
    type Error = UrlError;

    fn try_from(text: String) -> Result<Self, UrlError> {
        let Ok(uri) = Uri::parse(text.as_str()) else {
            return Err(UrlError::Malformed { text });
        };
        if !matches!(uri.scheme().as_str(), "http" | "https") {
            return Err(UrlError::Scheme { text });
        }
        if uri
            .authority()
            .is_none_or(|authority| authority.host().is_empty())
        {
            return Err(UrlError::NoHost { text });
        }
        Ok(Self(text))
    }
}

impl FromStr for Url {
    type Err = UrlError;

    fn from_str(text: &str) -> Result<Self, UrlError> {
        Self::try_from(text.to_owned())
    }
}

impl From<Url> for String {
    fn from(url: Url) -> Self {
        url.0
    }
}

impl fmt::Display for Url {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl JsonSchema for Url {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Url".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "uri", "pattern": PATTERN})
    }
}

/// Text that is not a [`Url`]. Board's schema check refuses such a record
/// first, with `board::record::invalid`, so these codes reach only Rust code
/// that reads a stored record; no command prints them.
#[derive(Debug, thiserror::Error)]
pub enum UrlError {
    #[error("{text:?} is not an absolute URI; write a URL such as https://example.com/page")]
    Malformed { text: String },
    #[error("{text:?} does not start with http:// or https:// in lower case")]
    Scheme { text: String },
    #[error("{text:?} names no host; write one after the //, such as https://example.com")]
    NoHost { text: String },
}

impl UrlError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Malformed { .. } => "board::url::malformed",
            Self::Scheme { .. } => "board::url::scheme",
            Self::NoHost { .. } => "board::url::no_host",
        }
    }
}

#[cfg(test)]
mod tests;

//! The envelope every `board --json` report prints, and the failure
//! document `board/failure/v1`, in the shape crumbkit prints them.

use std::collections::BTreeMap;

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The data of a report, which names the report's `schema`.
pub trait Data {
    /// The report's id, such as `board/put-report/v2`.
    const SCHEMA: &'static str;
}

/// A report: its `schema`, its `data`, the `failures` of the keys that did
/// not work, and `dry_run` when the run changed nothing. A listing's
/// `total`, `truncated` and `summary` are not read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report<D> {
    pub schema: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    pub data: D,
    /// The failures, when the report lists them. A command that takes
    /// several keys lists them even when there are none, as `[]`; others
    /// leave the field out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failures: Option<Vec<Failure>>,
}

/// What one run printed: the report of `D`, or the failure document,
/// told apart by `schema`. Reading one refuses any schema other than
/// `D`'s and `board/failure/v1`, naming the one it found, unless `data`
/// is refused first.
#[derive(Clone, Debug, PartialEq)]
pub enum Printed<D> {
    Report(Report<D>),
    Failure(Box<Failure>),
}

/// A `board/failure/v1` document, or one entry of a report's `failures`,
/// which has the same fields without `schema`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// The key the caller gave for the answer that failed, in a batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The position of `key` among the keys the caller gave, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// The values at fault, by name. crumbkit prints them in the order the
    /// failure gives them; this map orders them by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    pub recovery: Recovery,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Recovery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Failure {
    /// The failure document's id.
    pub const SCHEMA: &'static str = "board/failure/v1";
}

/// What the caller can do next, tagged by `kind`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Recovery {
    /// Run `call`; with `all`, once for each value of the field `all` names.
    Run {
        call: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        all: Option<String>,
    },
    /// Run the same command again later.
    Retry,
    /// Follow this advice.
    Hint { text: String },
    /// There is no next step, for this reason.
    None { reason: String },
}

/// A value at fault: crumbkit's `field_value`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Unsigned(u64),
    Text(String),
    List(Vec<FieldValue>),
    Object(BTreeMap<String, FieldValue>),
}

/// A place in a source, such as a file, that a failure points at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    /// The byte offset of the start of the span.
    pub offset: u64,
    /// The length of the span in bytes.
    pub length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<Excerpt>,
}

/// The lines of a source that a label's span covers, from `line`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excerpt {
    pub line: u64,
    pub text: String,
}

/// A diagnostic related to a failure's, to at most eight levels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub message: String,
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
}

/// How severe a related diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Advice,
}

/// The failure document as printed: `schema`, then the failure's fields.
#[derive(Serialize)]
struct Schemed<'a> {
    schema: &'static str,
    #[serde(flatten)]
    failure: &'a Failure,
}

/// A printed document's fields, before its `schema` says which it is.
#[derive(Deserialize)]
struct Either<D> {
    schema: String,
    #[serde(default)]
    dry_run: bool,
    #[serde(default = "none")]
    data: Option<D>,
    #[serde(default)]
    failures: Option<Vec<Failure>>,
    /// The failure the other fields make, if they make one.
    #[serde(flatten)]
    failure: Option<Failure>,
}

fn none<D>() -> Option<D> {
    None
}

impl<D: Serialize> Serialize for Printed<D> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Report(report) => report.serialize(serializer),
            Self::Failure(failure) => Schemed {
                schema: Failure::SCHEMA,
                failure: failure.as_ref(),
            }
            .serialize(serializer),
        }
    }
}

impl<'de, D: Data + Deserialize<'de>> Deserialize<'de> for Printed<D> {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        let Either {
            schema,
            dry_run,
            data,
            failures,
            failure,
        } = Either::deserialize(deserializer)?;
        if schema == Failure::SCHEMA {
            failure
                .map(|failure| Printed::Failure(Box::new(failure)))
                .ok_or_else(|| {
                    de::Error::custom("the failure document lacks code, message or recovery")
                })
        } else if schema == D::SCHEMA {
            let data = data.ok_or_else(|| de::Error::missing_field("data"))?;
            Ok(Printed::Report(Report {
                schema,
                dry_run,
                data,
                failures,
            }))
        } else {
            Err(de::Error::custom(format!(
                "the document's schema is {schema}, not {} or {}",
                D::SCHEMA,
                Failure::SCHEMA
            )))
        }
    }
}

#[cfg(test)]
mod tests;

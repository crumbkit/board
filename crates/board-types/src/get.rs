//! `board/get-report/v2`: each file a get read.

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::put::Location;
use crate::report::Data;

/// One file in the data of a `board/get-report/v2`: where it is, its blob
/// or `null` for a directory, its bytes, and its `record` or the `code`
/// that says why it is not one.
///
/// A reader that wants only the failures keeps the default `R`, which
/// skips each record unread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File<R = IgnoredAny> {
    pub location: Location,
    pub blob: Option<String>,
    #[serde(flatten)]
    pub content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<R>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl File {
    /// The report's id.
    pub const SCHEMA: &'static str = "board/get-report/v2";
}

/// A get report's data is each file read, in the order the paths were given.
impl<R> Data for Vec<File<R>> {
    const SCHEMA: &'static str = File::SCHEMA;
}

/// A file's stored bytes: `text` when they are UTF-8, else `base64`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    Text(String),
    Base64(String),
}

#[cfg(test)]
mod tests;

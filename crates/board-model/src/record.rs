//! Record types and the kind of every entry (docs/architecture.md, Boards and
//! entries; Records).

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::code::Code;
use crate::json::parse_json;
use crate::pointer::{POINTER_TYPE, Pointer, pointer_from_value};

/// The `$type` of `.board/board.json`.
pub const CONFIG_TYPE: &str = "board/config/v1";

/// The directory that holds schemas.
const TYPES_DIRECTORY: &str = ".board/types/";

/// A record's `$type`: `<namespace>/<kind>/v<n>`, where namespace and kind
/// are lowercase ASCII letters, digits and hyphens, and `n` is a positive
/// integer with no leading zero.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RecordType(String);

impl RecordType {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the type is in the `board` namespace, reserved for Board's own
    /// documents.
    pub fn is_board(&self) -> bool {
        self.0.starts_with("board/")
    }

    /// Where a board holds this type's schema, relative to its root.
    pub fn schema_path(&self) -> String {
        format!("{TYPES_DIRECTORY}{}.schema.json", self.0)
    }
}

fn well_formed(text: &str) -> bool {
    let name = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let mut parts = text.split('/');
    let (Some(namespace), Some(kind), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let version = version.strip_prefix('v').unwrap_or("");
    name(namespace)
        && name(kind)
        && !version.is_empty()
        && !version.starts_with('0')
        && version.bytes().all(|b| b.is_ascii_digit())
}

impl TryFrom<String> for RecordType {
    type Error = RecordTypeError;

    fn try_from(text: String) -> Result<Self, RecordTypeError> {
        if well_formed(&text) {
            Ok(Self(text))
        } else {
            Err(RecordTypeError::Malformed { text })
        }
    }
}

impl FromStr for RecordType {
    type Err = RecordTypeError;

    fn from_str(text: &str) -> Result<Self, RecordTypeError> {
        Self::try_from(text.to_owned())
    }
}

impl From<RecordType> for String {
    fn from(record_type: RecordType) -> Self {
        record_type.0
    }
}

impl fmt::Display for RecordType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A `$type` that is not of the form `<namespace>/<kind>/v<n>`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecordTypeError {
    #[error("record type {text:?} is not of the form <namespace>/<kind>/v<n>")]
    Malformed { text: String },
}

impl RecordTypeError {
    pub fn code(&self) -> Code {
        match self {
            Self::Malformed { .. } => Code::RecordBadType,
        }
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Record,
    Pointer,
    Schema,
    Json,
    File,
}

/// An entry's kind, what parsing found in it, and the code it is degraded
/// with, if any. Schema compilation, validation and references are checked
/// later, by the crates that own them.
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    pub kind: Kind,
    pub record_type: Option<RecordType>,
    pub pointer: Option<Pointer>,
    pub value: Option<Value>,
    pub code: Option<Code>,
}

impl Classified {
    fn of(kind: Kind) -> Self {
        Self {
            kind,
            record_type: None,
            pointer: None,
            value: None,
            code: None,
        }
    }

    fn degraded(kind: Kind, code: Code) -> Self {
        Self {
            code: Some(code),
            ..Self::of(kind)
        }
    }
}

/// Whether classifying the entry at `path` needs its bytes: only `.json`
/// files are read. The extension is matched exactly, so `x.JSON` is a file.
pub fn needs_bytes(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "json")
}

/// Classifies the entry at the `/`-separated physical `path`, holding
/// `bytes`, or `None` when it is over the size limit. Board's own types are
/// checked here; other records wait for their schema.
pub fn classify(path: &str, bytes: Option<&[u8]>) -> Classified {
    if !needs_bytes(path) {
        return Classified::of(Kind::File);
    }
    let schema = path.starts_with(TYPES_DIRECTORY) && path.ends_with(".schema.json");
    let parsed_kind = if schema { Kind::Schema } else { Kind::Json };
    let Some(bytes) = bytes else {
        return Classified::degraded(parsed_kind, Code::JsonTooLarge);
    };
    let value = match parse_json(bytes) {
        Ok(value) => value,
        Err(error) => return Classified::degraded(parsed_kind, error.code()),
    };
    if schema {
        let reserved = path.starts_with(&format!("{TYPES_DIRECTORY}board/"));
        return Classified {
            value: Some(value),
            code: reserved.then_some(Code::SchemaInvalid),
            ..Classified::of(Kind::Schema)
        };
    }
    let Some(tag) = value.get("$type") else {
        return Classified {
            value: Some(value),
            ..Classified::of(Kind::Json)
        };
    };
    let Some(record_type) = tag.as_str().and_then(|tag| tag.parse::<RecordType>().ok()) else {
        return Classified {
            value: Some(value),
            ..Classified::degraded(Kind::Record, Code::RecordBadType)
        };
    };
    let (kind, pointer, code) = match record_type.as_str() {
        POINTER_TYPE => match pointer_from_value(&value) {
            Ok(pointer) => (Kind::Pointer, Some(pointer), None),
            Err(error) => (Kind::Pointer, None, Some(error.code())),
        },
        CONFIG_TYPE => (Kind::Record, None, None),
        _ if record_type.is_board() => (Kind::Record, None, Some(Code::RecordUnknownType)),
        _ => (Kind::Record, None, None),
    };
    Classified {
        kind,
        record_type: Some(record_type),
        pointer,
        value: Some(value),
        code,
    }
}

#[cfg(test)]
mod tests;

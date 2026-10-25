//! Record types and the kind of every entry (docs/architecture.md, Boards and
//! entries; Records).

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::attachment::ATTACHMENT_TYPE;
use crate::code::Code;
use crate::effect::EFFECT_TYPES;
use crate::frontmatter::frontmatter;
use crate::json::parse_json;
use crate::pointer::{POINTER_TYPE, Pointer, pointer_from_value};

/// The `$type` of `.board/board.json`.
pub const CONFIG_TYPE: &str = "board/config/v1";

/// The `$type` of a plugin's `plugin.json`.
pub const MANIFEST_TYPE: &str = "board/plugin-manifest/v1";

/// The directory that holds schemas.
pub const TYPES_DIRECTORY: &str = ".board/types/";

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

    /// The type whose schema a board holds at `path`, relative to its root:
    /// the inverse of [`RecordType::schema_path`].
    pub fn of_schema(path: &str) -> Option<Self> {
        let name = path.strip_prefix(TYPES_DIRECTORY)?;
        name.strip_suffix(".schema.json")?.parse().ok()
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

/// What parsing found in an entry. Each kind carries exactly what it has.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Record {
        record_type: RecordType,
        value: Value,
    },
    Pointer {
        pointer: Pointer,
        value: Value,
    },
    Schema {
        value: Value,
    },
    Json {
        value: Value,
    },
    File,
    /// A `.json` entry, or a Markdown file's frontmatter, whose bytes could
    /// not be read as its kind needs: they do not parse, the `$type` is
    /// malformed, or the pointer is invalid. Its code says which.
    Unparsed(Kind),
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Self::Record,
        Self::Pointer,
        Self::Schema,
        Self::Json,
        Self::File,
    ];

    /// The name reports and the index give the kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Pointer => "pointer",
            Self::Schema => "schema",
            Self::Json => "json",
            Self::File => "file",
        }
    }
}

/// How an entry's path says to read its bytes. A path that is neither
/// `.json` nor `.md` has no role: it is a file, never read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Json,
    Schema,
    /// A schema in the reserved `board` namespace, which no board may define.
    ReservedSchema,
    /// A Markdown file, a file whose links are references, and a record
    /// when its frontmatter names a `$type`.
    Markdown,
}

impl Role {
    /// The role of the entry at the `/`-separated physical `path`. The
    /// extension is matched exactly, so `x.JSON` is a file.
    pub fn of(path: &str) -> Option<Self> {
        match Path::new(path).extension() {
            Some(extension) if extension == "md" => return Some(Self::Markdown),
            Some(extension) if extension == "json" => {}
            _ => return None,
        }
        if !(path.starts_with(TYPES_DIRECTORY) && path.ends_with(".schema.json")) {
            return Some(Self::Json);
        }
        if path.starts_with(&format!("{TYPES_DIRECTORY}board/")) {
            Some(Self::ReservedSchema)
        } else {
            Some(Self::Schema)
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Schema => "schema",
            Self::ReservedSchema => "reserved_schema",
            Self::Markdown => "markdown",
        }
    }

    /// Why `bytes` do not parse in this role, with the code: the JSON
    /// parser's message, or for a Markdown file its frontmatter's. `None`
    /// when they parse.
    pub fn parse_error(self, bytes: &[u8]) -> Option<(Code, String)> {
        match self {
            Self::Markdown => frontmatter(bytes)
                .err()
                .map(|error| (error.code(), error.to_string())),
            _ => parse_json(bytes)
                .err()
                .map(|error| (error.code(), error.to_string())),
        }
    }

    /// Classifies `bytes` read in this role, or `None` when they are over
    /// the size limit. Board's own types are checked here; other records
    /// wait for their schema.
    pub fn classify(self, bytes: Option<&[u8]>) -> Classified {
        if self == Self::Markdown {
            return markdown(bytes);
        }
        let parsed_kind = if self == Self::Json {
            Kind::Json
        } else {
            Kind::Schema
        };
        let Some(bytes) = bytes else {
            return Classified::unparsed(parsed_kind, Code::JsonTooLarge);
        };
        let value = match parse_json(bytes) {
            Ok(value) => value,
            Err(error) => return Classified::unparsed(parsed_kind, error.code()),
        };
        if self != Self::Json {
            return Classified {
                content: Content::Schema { value },
                code: (self == Self::ReservedSchema).then_some(Code::SchemaInvalid),
            };
        }
        let Some(tag) = value.get("$type") else {
            return Classified::ok(Content::Json { value });
        };
        let Some(record_type) = tag.as_str().and_then(|tag| tag.parse::<RecordType>().ok()) else {
            return Classified::unparsed(Kind::Record, Code::RecordBadType);
        };
        match record_type.as_str() {
            POINTER_TYPE => match pointer_from_value(&value) {
                Ok(pointer) => Classified::ok(Content::Pointer { pointer, value }),
                Err(error) => Classified::unparsed(Kind::Pointer, error.code()),
            },
            _ => Classified {
                code: (record_type.is_board()
                    && !matches!(
                        record_type.as_str(),
                        CONFIG_TYPE | MANIFEST_TYPE | ATTACHMENT_TYPE
                    )
                    && !EFFECT_TYPES.contains(&record_type.as_str()))
                .then_some(Code::RecordUnknownType),
                content: Content::Record { record_type, value },
            },
        }
    }
}

/// Classifies a Markdown file: a record when it has frontmatter that claims
/// a `$type`, else a file. A file over the size limit is a file.
fn markdown(bytes: Option<&[u8]>) -> Classified {
    match bytes.map(frontmatter) {
        None | Some(Ok(None)) => Classified::ok(Content::File),
        Some(Err(error)) => Classified::unparsed(Kind::File, error.code()),
        Some(Ok(Some(value))) => Classified::frontmatter(value),
    }
}

impl Content {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Record { .. } => Kind::Record,
            Self::Pointer { .. } => Kind::Pointer,
            Self::Schema { .. } => Kind::Schema,
            Self::Json { .. } => Kind::Json,
            Self::File => Kind::File,
            Self::Unparsed(kind) => *kind,
        }
    }
}

/// An entry's content and the code it is degraded with, if any. Content is
/// `Unparsed` only with a code. Schema compilation, validation and
/// references are checked later, by the crates that own them.
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    pub content: Content,
    pub code: Option<Code>,
}

impl Classified {
    pub fn kind(&self) -> Kind {
        self.content.kind()
    }

    /// Classifies a Markdown file whose frontmatter, a block that claims a
    /// `$type`, reads as `value`: a record of that type, or `bad_type` when
    /// it is not a mapping with a well-formed `$type`. Board's own types are
    /// JSON documents, so one named here is unknown.
    pub fn frontmatter(value: Value) -> Self {
        let tag = value.get("$type").and_then(Value::as_str);
        let Some(record_type) = tag.and_then(|tag| tag.parse::<RecordType>().ok()) else {
            return Self::unparsed(Kind::Record, Code::RecordBadType);
        };
        Self {
            code: record_type.is_board().then_some(Code::RecordUnknownType),
            content: Content::Record { record_type, value },
        }
    }

    fn ok(content: Content) -> Self {
        Self {
            content,
            code: None,
        }
    }

    fn unparsed(kind: Kind, code: Code) -> Self {
        Self {
            content: Content::Unparsed(kind),
            code: Some(code),
        }
    }
}

/// Whether the entry at `path` is a `.json` file, which Board reads as JSON.
pub fn is_json_file(path: &str) -> bool {
    Role::of(path).is_some_and(|role| role != Role::Markdown)
}

/// Classifies the entry at the `/`-separated physical `path`, holding
/// `bytes`, or `None` when it is over the size limit.
pub fn classify(path: &str, bytes: Option<&[u8]>) -> Classified {
    Role::of(path).map_or(Classified::ok(Content::File), |role| role.classify(bytes))
}

#[cfg(test)]
mod tests;

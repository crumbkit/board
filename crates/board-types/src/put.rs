//! `board/put-report/v2`: what a put wrote and removed and its commit, and
//! one line of `board put --batch`.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::report::Data;

/// The data of a `board/put-report/v2`, which `put`, `rm` and `type add`
/// print. `plugin enable` prints `board/plugin-enable-report/v2`, which this
/// type does not read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutReport {
    /// The paths a removal that ignores what names nothing found absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
    /// The commit the put made, or `null` when it made none.
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Removed>,
    pub writes: Vec<Write>,
}

impl PutReport {
    /// The report's id.
    pub const SCHEMA: &'static str = "board/put-report/v2";
}

impl Data for PutReport {
    const SCHEMA: &'static str = Self::SCHEMA;
}

/// One file a put wrote: its blob, the blob it replaced or `null` for a new
/// file, and what `board check` finds wrong with it, or `null`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Write {
    pub blob: String,
    pub degraded: Option<Degraded>,
    /// The logical path the file was written at.
    pub display: String,
    pub location: Location,
    pub previous: Option<String>,
}

/// Why a written file is degraded: its code, the JSON pointer at fault or
/// `null`, why, and how many failures the check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Degraded {
    pub code: String,
    pub failures: u64,
    pub pointer: Option<String>,
    pub reason: String,
}

/// One file a put removed, and the blob it held.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub blob: String,
    pub path: String,
}

/// Where a path resolved: the repository that holds it, the source read
/// (`worktree` or a commit), the path in its tree, the logical path shown,
/// and each pointer followed to get there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub display: String,
    pub path: String,
    pub pointers: Vec<Pointer>,
    pub repository: String,
    pub source: String,
}

/// A pointer followed on the way to a [`Location`], and the ref it follows
/// when it is a mount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<String>,
    pub path: String,
    pub repository: String,
    pub source: String,
}

/// One line of `board put --batch`, holding a record of type `R`.
///
/// `expect` is the blob the file must hold, `none` when it must be absent,
/// or left out to write whatever it holds. A `new` line names a folder,
/// ending in `/`, under which Board draws a new name. Unlike a report, a
/// line refuses a field it does not know, as Board does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchLine<R> {
    Record {
        path: String,
        expect: Option<String>,
        record: R,
    },
    Text {
        path: String,
        expect: Option<String>,
        text: String,
    },
    Remove {
        path: String,
        expect: Option<String>,
        ignore_unmatch: bool,
    },
    New {
        new: String,
        record: R,
    },
}

/// A batch line as written, before its fields are checked together.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine<R> {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    new: Option<String>,
    #[serde(default)]
    expect: Option<String>,
    record: Option<R>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    ignore_unmatch: bool,
}

/// A batch line as it is written: only the fields its form holds.
#[derive(Serialize)]
struct ShownLine<'a, R> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expect: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<&'a R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    remove: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    ignore_unmatch: bool,
}

impl<'a, R> ShownLine<'a, R> {
    /// A line at `path` with `expect`, holding nothing yet.
    const fn at(path: &'a str, expect: Option<&'a str>) -> Self {
        Self {
            path: Some(path),
            new: None,
            expect,
            record: None,
            text: None,
            remove: false,
            ignore_unmatch: false,
        }
    }
}

impl<R> TryFrom<RawLine<R>> for BatchLine<R> {
    type Error = &'static str;

    fn try_from(raw: RawLine<R>) -> Result<Self, Self::Error> {
        let RawLine {
            path,
            new,
            expect,
            record,
            text,
            remove,
            ignore_unmatch,
        } = raw;
        let path = match (path, new) {
            (Some(path), None) => path,
            (None, Some(new)) => {
                return match (expect, record, text, remove, ignore_unmatch) {
                    (None, Some(record), None, false, false) => Ok(Self::New { new, record }),
                    (Some(_), ..) | (None, None, ..) => {
                        Err("a new line holds a record and no expect, since its name is new")
                    }
                    (None, Some(_), _, false, true) => {
                        Err("it ignores an unmatched path and does not remove")
                    }
                    (None, Some(_), Some(_), ..) => Err("it holds both record and text"),
                    (None, Some(_), None, true, _) => {
                        Err("it removes and also holds a record or text")
                    }
                };
            }
            (Some(_), Some(_)) => return Err("it holds both path and new"),
            (None, None) => return Err("it holds no path or new"),
        };
        if ignore_unmatch && !remove {
            return Err("it ignores an unmatched path and does not remove");
        }
        match (record, text, remove) {
            (Some(record), None, false) => Ok(Self::Record {
                path,
                expect,
                record,
            }),
            (None, Some(text), false) => Ok(Self::Text { path, expect, text }),
            (None, None, true) => Ok(Self::Remove {
                path,
                expect,
                ignore_unmatch,
            }),
            (Some(_), Some(_), _) => Err("it holds both record and text"),
            (_, _, true) => Err("it removes and also holds a record or text"),
            (None, None, false) => Err("it holds no record or text and does not remove"),
        }
    }
}

impl<'de, R: Deserialize<'de>> Deserialize<'de> for BatchLine<R> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawLine::deserialize(deserializer)?;
        Self::try_from(raw).map_err(de::Error::custom)
    }
}

impl<R: Serialize> Serialize for BatchLine<R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let shown = match self {
            Self::Record {
                path,
                expect,
                record,
            } => ShownLine {
                record: Some(record),
                ..ShownLine::at(path, expect.as_deref())
            },
            Self::Text { path, expect, text } => ShownLine {
                text: Some(text),
                ..ShownLine::at(path, expect.as_deref())
            },
            Self::Remove {
                path,
                expect,
                ignore_unmatch,
            } => ShownLine {
                remove: true,
                ignore_unmatch: *ignore_unmatch,
                ..ShownLine::at(path, expect.as_deref())
            },
            Self::New { new, record } => ShownLine {
                path: None,
                new: Some(new),
                record: Some(record),
                ..ShownLine::at("", None)
            },
        };
        shown.serialize(serializer)
    }
}

#[cfg(test)]
mod tests;

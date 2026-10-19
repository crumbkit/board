//! Logical paths: the addresses agents and records use, read against the root
//! of the board that holds them (docs/architecture/paths.md, Logical paths).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::code::Code;

/// The longest logical path, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// The longest segment, in bytes. Most file systems refuse longer names.
pub const MAX_SEGMENT_BYTES: usize = 255;

/// An absolute logical path: `/` alone (the board root), or `/` followed by
/// segments separated by `/`. A segment is non-empty, at most
/// [`MAX_SEGMENT_BYTES`], holds no control character and is not `.` or `..`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LogicalPath(String);

impl LogicalPath {
    /// `/`, the root of a board.
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The segments in order; the root has none.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0
            .split('/')
            .skip(1)
            .filter(|segment| !segment.is_empty())
    }

    /// `text` read from this path, as `Path::join` reads it: an absolute
    /// `text` replaces it, `.`, `./` or `./.` is this path, and `x` or `./x`
    /// is `x` beneath it. One trailing `/` on a relative `text` is dropped, as
    /// a shell adds it to a directory. Any other `.` or `..` segment stays
    /// malformed.
    pub fn join(&self, text: &str) -> Result<Self, PathError> {
        let rest = text.strip_prefix("./").unwrap_or(text);
        match rest.strip_suffix('/').unwrap_or(rest) {
            _ if text.is_empty() || text.starts_with('/') => text.parse(),
            "." | "" => Ok(self.clone()),
            rest => format!("{}/{rest}", self.0.trim_end_matches('/')).parse(),
        }
    }

    /// The path of the first `depth` segments: the root for 0, the whole
    /// path for `depth` past its last segment.
    #[must_use]
    pub fn prefix(&self, depth: usize) -> Self {
        match self.0.match_indices('/').nth(depth) {
            Some((0, _)) => Self::root(),
            Some((end, _)) => Self(self.0[..end].to_owned()),
            None => self.clone(),
        }
    }
}

impl TryFrom<String> for LogicalPath {
    type Error = PathError;

    fn try_from(path: String) -> Result<Self, PathError> {
        match malformation(&path) {
            None => Ok(Self(path)),
            Some(reason) => Err(PathError::Malformed { path, reason }),
        }
    }
}

impl FromStr for LogicalPath {
    type Err = PathError;

    fn from_str(path: &str) -> Result<Self, PathError> {
        Self::try_from(path.to_owned())
    }
}

impl From<LogicalPath> for String {
    fn from(path: LogicalPath) -> Self {
        path.0
    }
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The first rule `path` breaks, if any. The whole-path length is checked
/// first, so a huge input is refused before its segments are scanned.
fn malformation(path: &str) -> Option<Malformation> {
    if path.len() > MAX_PATH_BYTES {
        return Some(Malformation::TooLong { bytes: path.len() });
    }
    let Some(rest) = path.strip_prefix('/') else {
        return Some(Malformation::NotAbsolute);
    };
    if rest.is_empty() {
        return None;
    }
    rest.split('/').find_map(|segment| {
        if segment.is_empty() {
            Some(Malformation::EmptySegment)
        } else if segment == "." || segment == ".." {
            Some(Malformation::DotSegment)
        } else if segment.len() > MAX_SEGMENT_BYTES {
            Some(Malformation::SegmentTooLong {
                bytes: segment.len(),
            })
        } else if segment.chars().any(char::is_control) {
            Some(Malformation::ControlCharacter)
        } else {
            None
        }
    })
}

/// Why a logical path is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Malformation {
    NotAbsolute,
    TooLong { bytes: usize },
    EmptySegment,
    DotSegment,
    SegmentTooLong { bytes: usize },
    ControlCharacter,
    NotUtf8,
}

impl fmt::Display for Malformation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute => formatter.write_str("it does not start with /"),
            Self::TooLong { bytes } => {
                write!(formatter, "it is {bytes} bytes, over {MAX_PATH_BYTES}")
            }
            Self::EmptySegment => formatter.write_str("it has an empty segment"),
            Self::DotSegment => formatter.write_str("it has a . or .. segment"),
            Self::SegmentTooLong { bytes } => write!(
                formatter,
                "it has a segment of {bytes} bytes, over {MAX_SEGMENT_BYTES}"
            ),
            Self::ControlCharacter => formatter.write_str("it holds a control character"),
            Self::NotUtf8 => formatter.write_str("it is not UTF-8"),
        }
    }
}

/// A refused logical path.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("logical path {path:?} is malformed: {reason}")]
    Malformed { path: String, reason: Malformation },
}

impl PathError {
    pub fn code(&self) -> Code {
        match self {
            Self::Malformed { .. } => Code::PathMalformed,
        }
    }
}

#[cfg(test)]
mod tests;

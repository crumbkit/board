//! Logical paths: the addresses agents and records use, read against the root
//! of the board that holds them (docs/architecture/paths.md, Logical paths).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::code::Code;

/// The longest logical path, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// The longest segment, in bytes. Most file systems refuse longer names.
pub const MAX_SEGMENT_BYTES: usize = 255;

/// The most symlinks one read follows: the fewest that an operating system
/// Board runs on follows (macOS), so that a working-tree read through the
/// file system reaches every file the tree's rule admits.
pub const MAX_SYMLINKS: usize = 32;

/// An absolute logical path: `/` alone (the board root), or `/` followed by
/// segments separated by `/`. A segment is non-empty, at most
/// [`MAX_SEGMENT_BYTES`], holds no character that [`is_hidden`] names and is
/// not `.` or `..`.
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

    /// The logical path of `physical`, a path from the board's root: `/`,
    /// then `physical` without a final `.json`.
    pub fn of_physical(physical: &str) -> Result<Self, PathError> {
        format!("/{}", physical.strip_suffix(".json").unwrap_or(physical)).parse()
    }

    /// `inside`, a path from the root of a board mounted at this path, as a
    /// path from this path's root.
    pub fn beneath(&self, inside: &Self) -> Result<Self, PathError> {
        match (self.as_str(), inside.as_str()) {
            ("/", _) => Ok(inside.clone()),
            (_, "/") => Ok(self.clone()),
            // Both paths are valid, so only the joined length can break a rule.
            (route, inside) => match format!("{route}{inside}") {
                path if path.len() > MAX_PATH_BYTES => Err(PathError::Malformed {
                    reason: Malformation::TooLong { bytes: path.len() },
                    path,
                }),
                path => Ok(Self(path)),
            },
        }
    }

    /// The directory that holds this path; the root's parent is the root.
    #[must_use]
    pub fn parent(&self) -> Self {
        match self.0.rsplit_once('/') {
            Some(("", _)) | None => Self::root(),
            Some((parent, _)) => Self(parent.to_owned()),
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
        } else {
            let char = segment.chars().find(|char| is_hidden(*char))?;
            Some(Malformation::HiddenCharacter { char })
        }
    })
}

/// Whether `char` can change how a text reads without showing as itself: a
/// control character, a Unicode format character such as a bidi control
/// (U+202E) or a zero-width space (U+200B), a line or paragraph separator
/// (U+2028, U+2029), or a Hangul filler. The zero-width joiner and
/// non-joiner (U+200D, U+200C) are admitted, because emoji sequences and
/// scripts such as Persian and Hindi need them in real names.
pub fn is_hidden(char: char) -> bool {
    match char {
        '\u{200c}' | '\u{200d}' => false,
        '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}' => true,
        // No ASCII character is a format character or a separator, so ASCII skips the
        // lookup, which copies a 41 KB const table per call in a debug build. Optimising
        // the Unicode crate does not help: the lookup is compiled into this crate.
        _ if char.is_ascii() => char.is_ascii_control(),
        _ => {
            char.is_control()
                || matches!(
                    get_general_category(char),
                    GeneralCategory::Format
                        | GeneralCategory::LineSeparator
                        | GeneralCategory::ParagraphSeparator
                )
        }
    }
}

/// `text` with each character that [`is_hidden`] names escaped, as
/// `\u{202e}`, so that printing it cannot hide or reorder what it says.
pub fn escaped(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    for char in text.chars() {
        if is_hidden(char) {
            shown.extend(char.escape_default());
        } else {
            shown.push(char);
        }
    }
    shown
}

/// Why a logical path is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Malformation {
    NotAbsolute,
    TooLong { bytes: usize },
    EmptySegment,
    DotSegment,
    SegmentTooLong { bytes: usize },
    HiddenCharacter { char: char },
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
            Self::HiddenCharacter { char } => write!(
                formatter,
                "it holds U+{:04X}, a character that can make a path look like another",
                u32::from(*char)
            ),
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

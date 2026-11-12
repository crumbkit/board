//! Moved prefixes: a prefix of logical paths whose records a renamed plugin
//! keeps under another prefix (docs/plugins/calling/commands.md, Renamed
//! plugins).

use serde::Deserialize;

use crate::path::LogicalPath;

/// A prefix records moved from, and the prefix they moved to. Neither is the
/// root, and neither lies beneath the other, so a path is under one of them
/// at most.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawMoved")]
pub struct Moved {
    from: LogicalPath,
    to: LogicalPath,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMoved {
    from: LogicalPath,
    to: LogicalPath,
}

impl TryFrom<RawMoved> for Moved {
    type Error = MovedError;

    fn try_from(raw: RawMoved) -> Result<Self, MovedError> {
        Self::new(raw.from, raw.to)
    }
}

impl Moved {
    pub fn new(from: LogicalPath, to: LogicalPath) -> Result<Self, MovedError> {
        let beneath = |inner: &LogicalPath, outer: &LogicalPath| {
            inner.segments().count() >= outer.segments().count()
                && inner.segments().zip(outer.segments()).all(|(a, b)| a == b)
        };
        if from == LogicalPath::root() || to == LogicalPath::root() {
            return Err(MovedError::Root);
        }
        if beneath(&from, &to) || beneath(&to, &from) {
            return Err(MovedError::Overlap { from, to });
        }
        Ok(Self { from, to })
    }

    pub fn from(&self) -> &LogicalPath {
        &self.from
    }

    pub fn to(&self) -> &LogicalPath {
        &self.to
    }

    /// `segments`, a path from a board's root, with the prefix it begins
    /// with replaced by the other one, in either direction; `None` when it
    /// begins with neither.
    pub fn swap<S: AsRef<str>>(&self, segments: &[S]) -> Option<Vec<String>> {
        [(&self.from, &self.to), (&self.to, &self.from)]
            .into_iter()
            .find_map(|(old, new)| {
                let old: Vec<&str> = old.segments().collect();
                let starts = segments.len() >= old.len()
                    && old.iter().zip(segments).all(|(a, b)| *a == b.as_ref());
                starts.then(|| {
                    (new.segments().map(str::to_owned))
                        .chain(segments[old.len()..].iter().map(|s| s.as_ref().to_owned()))
                        .collect()
                })
            })
    }
}

/// `segments` with the first of `moved` that applies swapped, as
/// [`Moved::swap`] swaps it.
pub fn swapped<S: AsRef<str>>(moved: &[Moved], segments: &[S]) -> Option<Vec<String>> {
    moved.iter().find_map(|moved| moved.swap(segments))
}

/// A moved prefix Board refuses.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MovedError {
    #[error("a moved prefix cannot be the root, which holds every path")]
    Root,
    #[error(
        "the moved prefixes {from} and {to} overlap; name two prefixes neither of which lies beneath the other"
    )]
    Overlap { from: LogicalPath, to: LogicalPath },
}

#[cfg(test)]
mod tests;

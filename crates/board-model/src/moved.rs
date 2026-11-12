//! Moved prefixes: a prefix of logical paths whose records a renamed plugin
//! keeps under another prefix (docs/plugins/calling/commands.md, Renamed
//! plugins).

use crate::path::LogicalPath;

/// A prefix records moved from, and the prefix they moved to. Neither is the
/// root, and neither lies beneath the other, so a path is under one of them
/// at most.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    from: LogicalPath,
    to: LogicalPath,
}

impl Moved {
    /// The moved prefix from `from` to `to`; `None` when either is the root
    /// or one lies beneath the other.
    pub fn new(from: LogicalPath, to: LogicalPath) -> Option<Self> {
        let beneath = |inner: &LogicalPath, outer: &LogicalPath| {
            inner.segments().count() >= outer.segments().count()
                && inner.segments().zip(outer.segments()).all(|(a, b)| a == b)
        };
        let root = LogicalPath::root();
        let apart = from != root && to != root && !beneath(&from, &to) && !beneath(&to, &from);
        apart.then_some(Self { from, to })
    }
}

/// `segments`, a path from a board's root, with the prefix it begins with
/// replaced by the other one, in either direction, for the first of `moved`
/// that applies; `None` when it begins with none of them. A path under no
/// prefix costs only the comparisons, with nothing allocated.
pub fn swapped<S: AsRef<str>>(moved: &[Moved], segments: &[S]) -> Option<Vec<String>> {
    let starts = |prefix: &LogicalPath| {
        let count = prefix.segments().count();
        (segments.len() >= count
            && prefix
                .segments()
                .zip(segments)
                .all(|(a, b)| a == b.as_ref()))
        .then_some(count)
    };
    moved.iter().find_map(|moved| {
        [(&moved.from, &moved.to), (&moved.to, &moved.from)]
            .into_iter()
            .find_map(|(old, new)| {
                let count = starts(old)?;
                Some(
                    (new.segments().map(str::to_owned))
                        .chain(segments[count..].iter().map(|s| s.as_ref().to_owned()))
                        .collect(),
                )
            })
    })
}

#[cfg(test)]
mod tests;

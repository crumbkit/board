//! How tree entries are addressed by logical path segments
//! (docs/architecture.md, Logical paths). Pure decisions over one directory.

use std::collections::HashMap;

use board_model::{Code, LogicalPath};

use crate::git::{Kind, RawEntry};

/// The segment that addresses an entry, if its name is UTF-8: a file's name
/// without `.json`, or any other entry's whole name. The segment may still
/// break the segment rules; parsing the child path decides that.
pub(crate) fn segment(entry: &RawEntry) -> Option<&str> {
    let name = std::str::from_utf8(&entry.name).ok()?;
    Some(match entry.kind {
        Kind::File => name.strip_suffix(".json").unwrap_or(name),
        Kind::Directory | Kind::Symlink | Kind::Submodule => name,
    })
}

/// The logical path of `segment` inside the directory at `parent`, if
/// `segment` is a valid segment and the path fits.
pub(crate) fn child(parent: &LogicalPath, segment: &str) -> Option<LogicalPath> {
    if segment.is_empty() {
        return None;
    }
    let path = if parent == &LogicalPath::root() {
        format!("/{segment}")
    } else {
        format!("{parent}/{segment}")
    };
    path.parse().ok()
}

/// Each entry's logical path and own status in one directory, in the same
/// order as `entries`. A name that is not a valid segment is
/// `path::malformed`; a segment shared by more than one entry is
/// `path::shadowed`; names equal but for ASCII case are
/// `path::case_collision`. The first that applies wins, in that order.
pub(crate) fn directory(
    parent: &LogicalPath,
    entries: &[RawEntry],
) -> Vec<(Option<LogicalPath>, Option<Code>)> {
    let mut segments: HashMap<&str, usize> = HashMap::new();
    let mut folded: HashMap<Vec<u8>, usize> = HashMap::new();
    for entry in entries {
        if let Some(segment) = segment(entry) {
            *segments.entry(segment).or_default() += 1;
        }
        *folded.entry(entry.name.to_ascii_lowercase()).or_default() += 1;
    }
    entries
        .iter()
        .map(|entry| {
            let Some(path) = segment(entry).and_then(|segment| child(parent, segment)) else {
                return (None, Some(Code::PathMalformed));
            };
            let code = if segment(entry).is_some_and(|segment| segments[segment] > 1) {
                Some(Code::PathShadowed)
            } else if folded[&entry.name.to_ascii_lowercase()] > 1 {
                Some(Code::PathCaseCollision)
            } else {
                None
            };
            (Some(path), code)
        })
        .collect()
}

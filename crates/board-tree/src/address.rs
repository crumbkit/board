//! How tree entries are addressed by logical path segments
//! (docs/architecture.md, Logical paths). Pure decisions over one directory.

use std::collections::{HashMap, HashSet};

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

/// What one segment names among the entries it matches: the sole entry, or a
/// record beside its directory of children. Anything else is shadowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named<'a> {
    Nothing,
    One(&'a RawEntry),
    RecordWithChildren {
        record: &'a RawEntry,
        children: &'a RawEntry,
    },
    Shadowed,
}

/// What `segment` names among `entries`.
pub(crate) fn named<'a>(entries: &'a [RawEntry], segment: &str) -> Named<'a> {
    classify(
        entries
            .iter()
            .filter(|entry| self::segment(entry) == Some(segment)),
    )
}

/// What the entries matching one segment name.
fn classify<'a>(mut matches: impl Iterator<Item = &'a RawEntry>) -> Named<'a> {
    match (matches.next(), matches.next(), matches.next()) {
        (None, _, _) => Named::Nothing,
        (Some(one), None, _) => Named::One(one),
        (Some(a), Some(b), None) => match (a.kind, b.kind) {
            (Kind::File, Kind::Directory) => Named::RecordWithChildren {
                record: a,
                children: b,
            },
            (Kind::Directory, Kind::File) => Named::RecordWithChildren {
                record: b,
                children: a,
            },
            _ => Named::Shadowed,
        },
        _ => Named::Shadowed,
    }
}

/// Each entry's logical path and own status in one directory, in the same
/// order as `entries`. The first that applies wins: a name that is not a
/// valid segment is `path::malformed`; a segment that names entries other
/// than one, or a record and its children, is `path::shadowed`; names equal
/// but for ASCII case are `path::case_collision`; a submodule is
/// `repo::submodule`.
pub(crate) fn directory(
    parent: &LogicalPath,
    entries: &[RawEntry],
) -> Vec<(Option<LogicalPath>, Option<Code>)> {
    let mut segments: HashMap<&str, Vec<&RawEntry>> = HashMap::new();
    let mut folded: HashMap<Vec<u8>, usize> = HashMap::new();
    for entry in entries {
        if let Some(segment) = segment(entry) {
            segments.entry(segment).or_default().push(entry);
        }
        *folded.entry(entry.name.to_ascii_lowercase()).or_default() += 1;
    }
    let shadowed: HashSet<&str> = segments
        .into_iter()
        .filter(|(_, group)| classify(group.iter().copied()) == Named::Shadowed)
        .map(|(segment, _)| segment)
        .collect();
    entries
        .iter()
        .map(|entry| {
            let Some((segment, path)) = segment(entry)
                .and_then(|segment| child(parent, segment).map(|path| (segment, path)))
            else {
                return (None, Some(Code::PathMalformed));
            };
            let code = if shadowed.contains(segment) {
                Some(Code::PathShadowed)
            } else if folded[&entry.name.to_ascii_lowercase()] > 1 {
                Some(Code::PathCaseCollision)
            } else if entry.kind == Kind::Submodule {
                Some(Code::RepoSubmodule)
            } else {
                None
            };
            (Some(path), code)
        })
        .collect()
}

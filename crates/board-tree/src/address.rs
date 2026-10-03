//! How tree entries are addressed by logical path segments
//! (docs/architecture.md, Logical paths). Pure decisions over one directory.

use std::collections::{HashMap, HashSet};

use board_model::{Code, LogicalPath};

use crate::tree::{Kind, RawEntry};

/// The segment that addresses an entry, if its name is UTF-8: a file's name
/// without `.json`, or any other entry's whole name. The segment may still
/// break the segment rules; parsing the child path decides that.
pub(crate) fn segment<D>(entry: &RawEntry<D>) -> Option<&str> {
    let name = std::str::from_utf8(&entry.name).ok()?;
    Some(match entry.kind {
        Kind::File(_) | Kind::Unreadable => name.strip_suffix(".json").unwrap_or(name),
        Kind::Directory(_) | Kind::Symlink(_) | Kind::Submodule => name,
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
#[derive(Debug)]
pub(crate) enum Named<'a, D> {
    Nothing,
    One(&'a RawEntry<D>),
    RecordWithChildren {
        record: &'a RawEntry<D>,
        children: &'a RawEntry<D>,
    },
    Shadowed,
}

/// What `segment` names among `entries`.
pub(crate) fn named<'a, D>(entries: &'a [RawEntry<D>], segment: &str) -> Named<'a, D> {
    classify(
        entries
            .iter()
            .filter(|entry| self::segment(entry) == Some(segment)),
    )
}

/// What the entries matching one segment name.
fn classify<'a, D>(mut matches: impl Iterator<Item = &'a RawEntry<D>>) -> Named<'a, D> {
    match (matches.next(), matches.next(), matches.next()) {
        (None, _, _) => Named::Nothing,
        (Some(one), None, _) => Named::One(one),
        (Some(a), Some(b), None) => match (&a.kind, &b.kind) {
            (Kind::File(_) | Kind::Unreadable, Kind::Directory(_)) => Named::RecordWithChildren {
                record: a,
                children: b,
            },
            (Kind::Directory(_), Kind::File(_) | Kind::Unreadable) => Named::RecordWithChildren {
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
/// valid segment is `path::malformed`; a segment that names more than one
/// entry, other than a record that is not a pointer beside its children, is
/// `path::shadowed`; names equal
/// but for ASCII case are `path::case_collision`; a submodule is
/// `repo::submodule`.
pub(crate) fn directory<D>(
    parent: &LogicalPath,
    entries: &[RawEntry<D>],
    mut is_pointer: impl FnMut(&RawEntry<D>) -> bool,
) -> Vec<(Option<LogicalPath>, Option<Code>)> {
    let mut segments: HashMap<&str, Vec<&RawEntry<D>>> = HashMap::new();
    let mut folded: HashMap<Vec<u8>, usize> = HashMap::new();
    for entry in entries {
        if let Some(segment) = segment(entry) {
            segments.entry(segment).or_default().push(entry);
        }
        *folded.entry(entry.name.to_ascii_lowercase()).or_default() += 1;
    }
    let shadowed: HashSet<&str> = segments
        .into_iter()
        .filter(|(_, group)| match classify(group.iter().copied()) {
            Named::Shadowed => true,
            Named::RecordWithChildren { record, .. } => is_pointer(record),
            Named::Nothing | Named::One(_) => false,
        })
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
            } else if matches!(entry.kind, Kind::Submodule) {
                Some(Code::RepoSubmodule)
            } else if matches!(entry.kind, Kind::Unreadable) {
                Some(Code::RepoUnreadable)
            } else {
                None
            };
            (Some(path), code)
        })
        .collect()
}

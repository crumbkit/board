//! How tree entries are addressed by logical path segments
//! (docs/architecture/paths.md, Logical paths). Pure decisions over one
//! directory, given what a record beside a directory holds.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use board_model::{Code, LogicalPath, ObjectId, Pointer, PointerError};

use crate::tree::{Kind, RawEntry};

/// The segment that addresses an entry, if its name is UTF-8: a file's name
/// without `.json`, or any other entry's whole name. The segment may still
/// break the segment rules; parsing the child path decides that.
pub(crate) fn segment<D>(entry: &RawEntry<D>) -> Option<&str> {
    let name = std::str::from_utf8(&entry.name).ok()?;
    Some(match entry.kind {
        Kind::File(_) | Kind::Unreadable | Kind::Unhashed(_) => {
            name.strip_suffix(".json").unwrap_or(name)
        }
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
/// record that does not shadow its directory of children beside it. Anything
/// else is shadowed.
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

/// What `segment` names among `entries`. `shadows` reads a record beside
/// its directory to tell whether it shadows the directory.
pub(crate) fn named<'a, D, E>(
    entries: &'a [RawEntry<D>],
    segment: &str,
    shadows: impl FnOnce(&RawEntry<D>) -> Result<bool, E>,
) -> Result<Named<'a, D>, E> {
    classify(
        entries
            .iter()
            .filter(|entry| self::segment(entry) == Some(segment)),
        shadows,
    )
}

/// What the entries matching one segment name.
pub(crate) fn classify<'a, D, E>(
    mut matches: impl Iterator<Item = &'a RawEntry<D>>,
    shadows: impl FnOnce(&RawEntry<D>) -> Result<bool, E>,
) -> Result<Named<'a, D>, E> {
    let (record, children) = match (matches.next(), matches.next(), matches.next()) {
        (None, _, _) => return Ok(Named::Nothing),
        (Some(one), None, _) => return Ok(Named::One(one)),
        (Some(a), Some(b), None) => match (&a.kind, &b.kind) {
            (Kind::File(_) | Kind::Unreadable | Kind::Unhashed(_), Kind::Directory(_)) => (a, b),
            (Kind::Directory(_), Kind::File(_) | Kind::Unreadable | Kind::Unhashed(_)) => (b, a),
            _ => return Ok(Named::Shadowed),
        },
        _ => return Ok(Named::Shadowed),
    };
    Ok(if shadows(record)? {
        Named::Shadowed
    } else {
        Named::RecordWithChildren { record, children }
    })
}

/// What each `.json` blob read during one run holds as a pointer, by blob id.
/// A blob id names its bytes, so an entry is true in every source, and it
/// lives only as long as the run that made it.
#[derive(Debug, Default)]
pub(crate) struct Pointers(HashMap<ObjectId, Parsed>);

/// What a record holds as a pointer: one, none, or why it is not a valid one.
pub(crate) type Parsed = Result<Option<Pointer>, Arc<PointerError>>;

impl Pointers {
    /// What the file `entry` holds as a pointer, parsed at most once per
    /// blob. `read` gives its bytes, or `None` when it is too large to be one.
    pub(crate) fn parse<D, E>(
        &mut self,
        entry: &RawEntry<D>,
        read: impl FnOnce() -> Result<Option<Vec<u8>>, E>,
    ) -> Result<Parsed, E> {
        let parse = || -> Result<Parsed, E> {
            Ok(read()?.map_or(Ok(None), |bytes| {
                board_model::pointer(&bytes).map_err(Arc::new)
            }))
        };
        let Kind::File(blob) = entry.kind else {
            return parse();
        };
        Ok(match self.0.entry(blob) {
            Entry::Occupied(known) => known.get().clone(),
            Entry::Vacant(unknown) => unknown.insert(parse()?).clone(),
        })
    }

    /// Whether `record`, beside a directory, shadows it: it is a readable
    /// record of the pointer type, valid or not.
    pub(crate) fn shadows<D, E>(
        &mut self,
        record: &RawEntry<D>,
        read: impl FnOnce() -> Result<Option<Vec<u8>>, E>,
    ) -> Result<bool, E> {
        Ok(!matches!(self.parse(record, read)?, Ok(None)))
    }
}

/// Each entry's logical path and own status in one directory, in the same
/// order as `entries`. The first that applies wins: a name that is not a
/// valid segment is `path::malformed`; a segment that names more than one
/// entry, other than a record that is not a pointer beside its children, is
/// `path::shadowed`, and one whose record cannot be read to tell is
/// `repo::unreadable`; names equal but for ASCII case are
/// `path::case_collision`; a submodule is `repo::submodule`. `shadows`
/// reads a record beside its children, as [`named`] does. An unhashed file
/// gets no code of its own: only a walk reports one, and a walk hashes
/// every file it lists.
pub(crate) fn directory<D, E>(
    parent: &LogicalPath,
    entries: &[RawEntry<D>],
    mut shadows: impl FnMut(&RawEntry<D>) -> Result<bool, E>,
) -> Vec<(Option<LogicalPath>, Option<Code>)> {
    let mut segments: HashMap<&str, Vec<&RawEntry<D>>> = HashMap::new();
    let mut folded: HashMap<Vec<u8>, usize> = HashMap::new();
    for entry in entries {
        if let Some(segment) = segment(entry) {
            segments.entry(segment).or_default().push(entry);
        }
        *folded.entry(entry.name.to_ascii_lowercase()).or_default() += 1;
    }
    let grouped: HashMap<&str, Code> = segments
        .into_iter()
        .filter_map(|(segment, group)| {
            let code = match classify(group.iter().copied(), &mut shadows) {
                Ok(Named::Shadowed) => Code::PathShadowed,
                Err(_) => Code::RepoUnreadable,
                Ok(Named::Nothing | Named::One(_) | Named::RecordWithChildren { .. }) => {
                    return None;
                }
            };
            Some((segment, code))
        })
        .collect();
    entries
        .iter()
        .map(|entry| {
            let Some((segment, path)) = segment(entry)
                .and_then(|segment| child(parent, segment).map(|path| (segment, path)))
            else {
                return (None, Some(Code::PathMalformed));
            };
            let code = if let Some(code) = grouped.get(segment) {
                Some(*code)
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

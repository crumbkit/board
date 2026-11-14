//! The three-way merge of git trees that `integrate` makes from the common
//! ancestor, the local tree and the fetched one (the merge table in
//! `/changes/board-writes-and-sync/lifecycle`). It reads objects and writes
//! only the merged tree objects: no file, ref or commit.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{
    CONFLICTS_DIRECTORY, Mount, ObjectId, Pointer, Side, TYPES_DIRECTORY, can_hold_pointer,
    is_json_file, pointer, repinned,
};
use gix::bstr::BString;
use gix::objs::tree::{Entry, EntryKind, EntryMode};

use crate::git::{GitError, Repository, model_id};

/// What a merge made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Merged {
    pub(crate) tree: ObjectId,
    pub(crate) conflicts: Vec<Conflict>,
    pub(crate) pins: Vec<PinResolution>,
    /// Schemas both sides changed; the remote one is in `tree`, and a later
    /// step lists the local records that fail it.
    pub(crate) schemas: Vec<SchemaClash>,
}

/// A path both sides changed in ways that cannot both stand. The ids are
/// those of the entries held (a blob, link or submodule commit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Conflict {
    /// Relative to the tree's root, `/`-separated.
    pub(crate) path: String,
    /// The blob the common ancestor held, if it held one.
    pub(crate) base: Option<ObjectId>,
    pub(crate) kind: ConflictKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConflictKind {
    /// Both edited, or both added, different content. The remote wins.
    BothChanged { local: ObjectId, remote: ObjectId },
    /// One side edited and the other deleted; the edit wins.
    EditDelete { edited: ObjectId, by: Side },
    /// One side holds a file and the other a directory. The remote wins,
    /// which is the directory when `by` is the local side.
    FileDirectory { file: ObjectId, by: Side },
}

/// A mount pointer both sides moved, with nothing else changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinResolution {
    pub path: String,
    pub local: ObjectId,
    pub remote: ObjectId,
    pub taken: Side,
    pub basis: PinBasis,
}

impl PinBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Descendant => "descendant",
            Self::Unrelated => "unrelated",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether commit `candidate` has `of` among its ancestors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ancestry {
    Descends,
    DoesNot,
    /// A commit could not be read, so nothing can be said.
    Unknown,
}

/// Why a pin side was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinBasis {
    /// The side taken descends from the other.
    Descendant,
    /// Neither descends from the other, so the remote was taken.
    Unrelated,
    /// Ancestry could not be read, so the local pin stayed.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SchemaClash {
    pub(crate) path: String,
    pub(crate) local: ObjectId,
    pub(crate) remote: ObjectId,
}

/// One tree entry: what a name holds in a directory.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Slot {
    id: gix::ObjectId,
    mode: EntryMode,
}

impl Slot {
    fn is_tree(self) -> bool {
        self.mode.is_tree()
    }

    fn object(self) -> ObjectId {
        model_id(&self.id)
    }

    /// The blob the slot holds, if it holds a file or a link.
    fn blob(self) -> Option<ObjectId> {
        self.mode.is_blob_or_symlink().then(|| model_id(&self.id))
    }
}

/// Merges the trees `local` and `remote` against their common ancestor
/// `base`, `None` when they have none. `descends(mount, a, b)` says whether commit
/// `a` has `b` among its ancestors in the repository `mount` names; it
/// settles two moved mount pins.
pub(crate) fn merge_trees(
    repository: &Repository,
    base: Option<gix::ObjectId>,
    local: gix::ObjectId,
    remote: gix::ObjectId,
    descends: &dyn Fn(&Mount, ObjectId, ObjectId) -> Ancestry,
) -> Result<Merged, GitError> {
    let mut merge = Merge {
        repository,
        descends,
        conflicts: Vec::new(),
        pins: Vec::new(),
        schemas: Vec::new(),
    };
    let tree = merge.directory("", base, Some(local), Some(remote))?;
    let tree = match tree {
        Some(tree) => tree,
        None => merge.write(Vec::new())?,
    };
    let tree = model_id(&tree);
    Ok(Merged {
        tree,
        conflicts: merge.conflicts,
        pins: merge.pins,
        schemas: merge.schemas,
    })
}

struct Merge<'a> {
    repository: &'a Repository,
    descends: &'a dyn Fn(&Mount, ObjectId, ObjectId) -> Ancestry,
    conflicts: Vec<Conflict>,
    pins: Vec<PinResolution>,
    schemas: Vec<SchemaClash>,
}

impl Merge<'_> {
    /// The merge of one directory, `None` when nothing is left in it.
    /// `prefix` is empty or ends in `/`.
    fn directory(
        &mut self,
        prefix: &str,
        base: Option<gix::ObjectId>,
        local: Option<gix::ObjectId>,
        remote: Option<gix::ObjectId>,
    ) -> Result<Option<gix::ObjectId>, GitError> {
        // Identical subtrees are never read.
        if local == remote || base == remote {
            return Ok(local);
        }
        if base == local {
            return Ok(remote);
        }
        let (base, local, remote) = (
            self.entries(base)?,
            self.entries(local)?,
            self.entries(remote)?,
        );
        let names: BTreeSet<&BString> = base
            .keys()
            .chain(local.keys())
            .chain(remote.keys())
            .collect();
        let mut merged = Vec::new();
        for name in names {
            let path = format!("{prefix}{name}");
            let slot = self.slot(
                &path,
                base.get(name).copied(),
                local.get(name).copied(),
                remote.get(name).copied(),
            )?;
            if let Some(Slot { id, mode }) = slot {
                merged.push(Entry {
                    mode,
                    filename: name.clone(),
                    oid: id,
                });
            }
        }
        if merged.is_empty() {
            return Ok(None);
        }
        self.write(merged).map(Some)
    }

    /// What `path` holds after the merge.
    fn slot(
        &mut self,
        path: &str,
        base: Option<Slot>,
        local: Option<Slot>,
        remote: Option<Slot>,
    ) -> Result<Option<Slot>, GitError> {
        if local == remote || base == remote {
            return Ok(local);
        }
        if base == local {
            return Ok(remote);
        }
        let tree_of = |slot: Option<Slot>| slot.filter(|slot| slot.is_tree()).map(|slot| slot.id);
        if [local, remote].iter().flatten().all(|slot| slot.is_tree()) {
            let prefix = format!("{path}/");
            let id = self.directory(&prefix, tree_of(base), tree_of(local), tree_of(remote))?;
            return Ok(id.map(|id| Slot {
                id,
                mode: EntryKind::Tree.into(),
            }));
        }
        if path.starts_with(CONFLICTS_DIRECTORY) {
            // A conflict record is never the subject of another: the
            // remote's stands, and so does the only one there is.
            return Ok(remote.or(local));
        }
        let (kept, kind) = match (local, remote) {
            (Some(file), Some(remote)) if remote.is_tree() => (
                Some(remote),
                ConflictKind::FileDirectory {
                    file: file.object(),
                    by: Side::Local,
                },
            ),
            (Some(local), Some(file)) if local.is_tree() => (
                Some(file),
                ConflictKind::FileDirectory {
                    file: file.object(),
                    by: Side::Remote,
                },
            ),
            (Some(local), Some(remote)) if local.id == remote.id && !local.mode.is_commit() => {
                return Ok(Some(remote));
            }
            (Some(local), Some(remote)) => {
                if let Some(taken) = self.pin(path, local, remote)? {
                    return Ok(Some(taken));
                }
                if let Some(merged) = self.text(path, base, local, remote)? {
                    return Ok(Some(merged));
                }
                self.flag_schema(path, local, remote);
                let kind = ConflictKind::BothChanged {
                    local: local.object(),
                    remote: remote.object(),
                };
                (Some(remote), kind)
            }
            (Some(edited), None) => (
                Some(edited),
                ConflictKind::EditDelete {
                    edited: edited.object(),
                    by: Side::Local,
                },
            ),
            (None, Some(edited)) => (
                Some(edited),
                ConflictKind::EditDelete {
                    edited: edited.object(),
                    by: Side::Remote,
                },
            ),
            (None, None) => return Ok(None),
        };
        self.conflicts.push(Conflict {
            path: path.to_owned(),
            base: base.and_then(Slot::blob),
            kind,
        });
        Ok(kept)
    }

    /// The text file both sides changed, merged line by line from `base`,
    /// when no hunk of one side overlaps a hunk of the other, or when the
    /// only hunks that meet are insertions at one point, which both stay
    /// in ([`insertions_together`]). A file [`merges_by_line`] excludes, a
    /// file that is not text and a merge with other overlapping hunks give
    /// `None`.
    fn text(
        &self,
        path: &str,
        base: Option<Slot>,
        local: Slot,
        remote: Slot,
    ) -> Result<Option<Slot>, GitError> {
        if !merges_by_line(path)
            || local.mode != remote.mode
            || !local.mode.is_blob()
            || base.is_some_and(|base| !base.mode.is_blob())
        {
            return Ok(None);
        }
        let read = |id: gix::ObjectId| self.repository.read_blob(model_id(&id));
        let (Some(ours), Some(theirs)) = (read(local.id)?, read(remote.id)?) else {
            return Ok(None);
        };
        let Some(ancestor) = base.map_or(Ok(Some(Vec::new())), |base| read(base.id))? else {
            return Ok(None);
        };
        if [&ours, &theirs, &ancestor]
            .iter()
            .any(|bytes| bytes.contains(&0))
        {
            return Ok(None);
        }
        let mut merged = Vec::new();
        let mut input = gix::diff::blob::InternedInput::new(&[][..], &[]);
        let outcome = gix::merge::blob::builtin_driver::text(
            &mut merged,
            &mut input,
            gix::merge::blob::builtin_driver::text::Labels::default(),
            &ours,
            &ancestor,
            &theirs,
            gix::merge::blob::builtin_driver::text::Options::default(),
        );
        if outcome != gix::merge::blob::Resolution::Complete {
            let Some(together) = insertions_together(&ancestor, &ours, &theirs) else {
                return Ok(None);
            };
            merged = together;
        }
        let id = self
            .repository
            .git()
            .write_blob(&merged)
            .map_err(|e| self.unreadable(e))?;
        Ok(Some(Slot {
            id: id.detach(),
            mode: local.mode,
        }))
    }

    /// The pin to keep when `local` and `remote` are mount pointers that
    /// differ only in their commit.
    fn pin(&mut self, path: &str, local: Slot, remote: Slot) -> Result<Option<Slot>, GitError> {
        if !can_hold_pointer(path) {
            return Ok(None);
        }
        let (Some((local_mount, local_bytes)), Some((remote_mount, remote_bytes))) =
            (self.pinned(local)?, self.pinned(remote)?)
        else {
            return Ok(None);
        };
        let (local_commit, remote_commit) = (local_mount.commit, remote_mount.commit);
        let same_but_commit = matches!(
            (repinned(&local_bytes, local_commit), repinned(&remote_bytes, local_commit)),
            (Ok(local), Ok(remote)) if local == remote
        );
        if !same_but_commit {
            return Ok(None);
        }
        let (taken, basis) = match (self.descends)(&remote_mount, remote_commit, local_commit) {
            Ancestry::Descends => (Side::Remote, PinBasis::Descendant),
            Ancestry::Unknown => (Side::Local, PinBasis::Unknown),
            Ancestry::DoesNot => match (self.descends)(&local_mount, local_commit, remote_commit) {
                Ancestry::Descends => (Side::Local, PinBasis::Descendant),
                Ancestry::DoesNot => (Side::Remote, PinBasis::Unrelated),
                Ancestry::Unknown => (Side::Local, PinBasis::Unknown),
            },
        };
        self.pins.push(PinResolution {
            path: path.to_owned(),
            local: local_commit,
            remote: remote_commit,
            taken,
            basis,
        });
        Ok(Some(match taken {
            Side::Local => local,
            Side::Remote => remote,
        }))
    }

    /// The mount and bytes of the mount pointer in `slot`.
    fn pinned(&self, slot: Slot) -> Result<Option<(Mount, Vec<u8>)>, GitError> {
        let Some(blob) = slot.blob() else {
            return Ok(None);
        };
        let Some(bytes) = self.repository.read_blob(blob)? else {
            return Ok(None);
        };
        Ok(match pointer(&bytes) {
            Ok(Some(Pointer::Mount(mount))) => Some((mount, bytes)),
            _ => None,
        })
    }

    fn flag_schema(&mut self, path: &str, local: Slot, remote: Slot) {
        if !path.starts_with(TYPES_DIRECTORY) || !path.ends_with(".schema.json") {
            return;
        }
        if let (Some(local), Some(remote)) = (local.blob(), remote.blob()) {
            self.schemas.push(SchemaClash {
                path: path.to_owned(),
                local,
                remote,
            });
        }
    }

    fn entries(&self, tree: Option<gix::ObjectId>) -> Result<BTreeMap<BString, Slot>, GitError> {
        let Some(tree) = tree else {
            return Ok(BTreeMap::new());
        };
        let tree = self
            .repository
            .git()
            .find_tree(tree)
            .map_err(|e| self.unreadable(e))?;
        let decoded = tree.decode().map_err(|e| self.unreadable(e))?;
        Ok(decoded
            .entries
            .iter()
            .map(|entry| {
                let slot = Slot {
                    id: entry.oid.to_owned(),
                    mode: entry.mode,
                };
                (entry.filename.to_owned(), slot)
            })
            .collect())
    }

    fn write(&self, mut entries: Vec<Entry>) -> Result<gix::ObjectId, GitError> {
        entries.sort();
        let id = self
            .repository
            .git()
            .write_object(gix::objs::Tree { entries })
            .map_err(|e| self.unreadable(e))?;
        Ok(id.detach())
    }

    fn unreadable(&self, source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> GitError {
        GitError::Unreadable {
            repository: self.repository.path().to_path_buf(),
            source: source.into(),
        }
    }
}

/// Whether the file at `path` may be merged line by line. A JSON record
/// (which includes every mount pointer, since only a plain `.json` file
/// can hold one) and a `.gitattributes` file take one side whole, because
/// a union of their lines can be invalid.
fn merges_by_line(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    !is_json_file(path) && name != ".gitattributes"
}

/// One change to a file's lines: the lines `before` in the ancestor became
/// the lines `after` in a side.
struct Hunk {
    before: std::ops::Range<u32>,
    after: std::ops::Range<u32>,
}

/// The line changes from `ancestor` to `side`, in order.
fn hunks(ancestor: &[u8], side: &[u8]) -> Vec<Hunk> {
    let input = gix::diff::blob::InternedInput::new(ancestor, side);
    let mut diff = gix::diff::blob::Diff::compute(gix::diff::blob::Algorithm::Myers, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| Hunk {
            before: hunk.before,
            after: hunk.after,
        })
        .collect()
}

/// Whether two hunks overlap or are adjacent in the ancestor.
fn touches(a: &Hunk, b: &Hunk) -> bool {
    a.before.start <= b.before.end && b.before.start <= a.before.end
}

/// The merge of `ours` and `theirs` from `ancestor` that keeps the
/// insertions both made at one point, `theirs` first and then `ours`.
/// Every other change must stand clear of the other side's: a change that
/// overlaps or is adjacent to one of the other side, other than two pure
/// insertions at the same line, gives `None`, and so does a merge with no
/// such pair of insertions, which `gix`'s merge settles. A line is kept with
/// its newline, and the remote's insertion must end in one, or the two
/// insertions would join into one line.
fn insertions_together(ancestor: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    fn lines(bytes: &[u8]) -> Vec<&[u8]> {
        bytes.split_inclusive(|b| *b == b'\n').collect()
    }
    let (base, ours_lines, theirs_lines) = (lines(ancestor), lines(ours), lines(theirs));
    let (ours_hunks, theirs_hunks) = (hunks(ancestor, ours), hunks(ancestor, theirs));
    let copy = |lines: &[&[u8]], range: &std::ops::Range<u32>, out: &mut Vec<u8>| {
        for line in &lines[range.start as usize..range.end as usize] {
            out.extend_from_slice(line);
        }
    };
    let mut merged = Vec::new();
    let mut at = 0;
    let (mut o, mut t) = (0, 0);
    let mut inserted = false;
    while o < ours_hunks.len() || t < theirs_hunks.len() {
        let (mine, other) = (ours_hunks.get(o), theirs_hunks.get(t));
        if let (Some(mine), Some(other)) = (mine, other)
            && touches(mine, other)
        {
            let same_point = mine.before.is_empty()
                && other.before.is_empty()
                && mine.before.start == other.before.start;
            let chained = ours_hunks
                .get(o + 1)
                .is_some_and(|next| touches(next, other))
                || theirs_hunks
                    .get(t + 1)
                    .is_some_and(|next| touches(next, mine));
            let open_ended = theirs_lines[other.after.start as usize..other.after.end as usize]
                .last()
                .is_some_and(|line| !line.ends_with(b"\n"));
            if !same_point || chained || open_ended {
                return None;
            }
            copy(&base, &(at..mine.before.start), &mut merged);
            copy(&theirs_lines, &other.after, &mut merged);
            let same = theirs_lines[other.after.start as usize..other.after.end as usize]
                == ours_lines[mine.after.start as usize..mine.after.end as usize];
            if !same {
                copy(&ours_lines, &mine.after, &mut merged);
            }
            at = mine.before.start;
            inserted = true;
            o += 1;
            t += 1;
            continue;
        }
        let from_ours = match (mine, other) {
            (Some(mine), Some(other)) => mine.before.start <= other.before.start,
            (mine, _) => mine.is_some(),
        };
        let (hunk, lines) = if from_ours {
            o += 1;
            (mine?, &ours_lines)
        } else {
            t += 1;
            (other?, &theirs_lines)
        };
        copy(&base, &(at..hunk.before.start), &mut merged);
        copy(lines, &hunk.after, &mut merged);
        at = hunk.before.end;
    }
    copy(&base, &(at..u32::try_from(base.len()).ok()?), &mut merged);
    inserted.then_some(merged)
}

#[cfg(test)]
mod tests;

//! Trees as the index keeps them: each tree once, by id, with its entries
//! addressed (docs/architecture/index.md). Addressing a tree's entries
//! depends only on their names and the records beside directories, so it is
//! a fact about the tree's content.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use board_model::{Code, LogicalPath, ObjectId, Source};

use crate::address::{self, Pointers};
use crate::git::{GitError, Repository, Sizes};
use crate::tree::{Indexed, Kind, RawEntry, Tree};
use crate::walk::{Entry, walk_in};

/// One tree's entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRows {
    pub id: ObjectId,
    pub rows: Vec<TreeRow>,
}

/// One entry of a tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// The entry's name, exactly as git records it.
    pub name: Vec<u8>,
    /// git's mode: `0o40000` for a tree, `0o100644` for a file, and so on.
    pub mode: u32,
    pub object: ObjectId,
    /// A file's or symlink's size in bytes, when its object can be read.
    pub size: Option<u64>,
    /// The logical segment that addresses it, unless its name is malformed.
    pub segment: Option<String>,
    /// The first problem with its name among its siblings.
    pub code: Option<Code>,
}

impl TreeRow {
    pub const TREE_MODE: u32 = 0o40_000;
    pub const LINK_MODE: u32 = 0o120_000;
    pub const SUBMODULE_MODE: u32 = 0o160_000;
}

/// Given a tree's id, its rows as [`Repository::trees`] gave them, or `None`
/// when they are not known.
pub type Rows<'r> = &'r mut dyn FnMut(ObjectId) -> Option<Vec<TreeRow>>;

/// A commit's tree as stored rows: walking it gives exactly what walking
/// the commit gives, reading from the repository only symlinks' text.
struct StoredTree<'r> {
    repository: &'r Repository,
    commit: ObjectId,
    root: ObjectId,
    rows: Rows<'r>,
    /// Each listed tree's entries and rows, by tree, so a tree is fetched
    /// once however often a walk or a symlink looks in it, and what was
    /// listed is addressed by its rows.
    listed: HashMap<ObjectId, Listed>,
}

/// A stored tree's entries as a walk reads them, and the rows they came from.
type Listed = (Indexed<ObjectId>, Vec<TreeRow>);

impl Tree for StoredTree<'_> {
    type Directory = ObjectId;

    fn repository(&self) -> &Path {
        self.repository.path()
    }

    fn source(&self) -> Source {
        Source::Commit(self.commit)
    }

    fn root(&self) -> ObjectId {
        self.root
    }

    fn list(&mut self, directory: &ObjectId) -> Result<Rc<[RawEntry<ObjectId>]>, GitError> {
        if let Some((listed, _)) = self.listed.get(directory) {
            return Ok(Rc::clone(&listed.entries));
        }
        let Some(rows) = (self.rows)(*directory).or_else(|| is_empty(*directory).then(Vec::new))
        else {
            return Err(GitError::Object {
                repository: self.repository.path().to_path_buf(),
                id: *directory,
                source: "the tree is not in the index".into(),
            });
        };
        let entries: Vec<_> = rows
            .iter()
            .map(|row| RawEntry {
                name: row.name.clone(),
                kind: match row.mode {
                    TreeRow::TREE_MODE => Kind::Directory(row.object),
                    TreeRow::LINK_MODE => Kind::Symlink(row.object),
                    TreeRow::SUBMODULE_MODE => Kind::Submodule,
                    _ => Kind::File(row.object),
                },
                stat: None,
                size: row.size,
            })
            .collect();
        let entries: Rc<[_]> = entries.into();
        self.listed
            .insert(*directory, (Indexed::new(Rc::clone(&entries)), rows));
        Ok(entries)
    }

    fn alike(
        &mut self,
        directory: &ObjectId,
        name: &[u8],
        segment: Option<&str>,
    ) -> Result<Vec<RawEntry<ObjectId>>, GitError> {
        self.list(directory)?;
        Ok(self.listed[directory].0.alike(name, segment))
    }

    /// Only a symlink is read: the stored codes say whether a record
    /// shadows its directory.
    fn read(
        &mut self,
        _directory: &ObjectId,
        entry: &RawEntry<ObjectId>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        match entry.kind {
            Kind::Symlink(id) => self.repository.read_blob(id),
            _ => Ok(None),
        }
    }

    /// The stored segments and codes: addressing a tree depends only on it,
    /// so only the whole path, which may grow too long, is checked again.
    fn address(
        &mut self,
        directory: &ObjectId,
        parent: &LogicalPath,
        entries: &[RawEntry<ObjectId>],
    ) -> Vec<(Option<LogicalPath>, Option<Code>)> {
        let rows = self
            .listed
            .get(directory)
            .map_or(&[][..], |(_, rows)| rows.as_slice());
        entries
            .iter()
            .zip(rows)
            .map(|(_, row)| {
                match row
                    .segment
                    .as_deref()
                    .and_then(|segment| address::child(parent, segment))
                {
                    Some(logical) => (Some(logical), row.code),
                    None => (None, Some(Code::PathMalformed)),
                }
            })
            .collect()
    }
}

/// Whether `id` names git's empty tree. It has no rows to store, so a
/// cache could not tell it apart from a tree it never listed.
fn is_empty(id: ObjectId) -> bool {
    gix::ObjectId::try_from(id.as_bytes()).is_ok_and(|id| id.is_empty_tree())
}

/// Walks the stored rows of `commit`'s tree `root` in `repository`, as
/// [`Repository::walk`] walks the commit: a tree whose rows are not known
/// is one degraded entry, as an unreadable one is.
pub fn walk_stored(
    repository: &Repository,
    commit: ObjectId,
    root: ObjectId,
    rows: Rows<'_>,
) -> Result<Vec<Entry>, GitError> {
    walk_in(&mut StoredTree {
        repository,
        commit,
        root,
        rows,
        listed: HashMap::new(),
    })
}

impl Repository {
    /// The root tree of the commit `HEAD` names, or `None` before the first
    /// commit. A detached `HEAD` names its commit directly.
    pub fn head_tree(&self) -> Result<Option<ObjectId>, GitError> {
        let Some(commit) = self.head()? else {
            return Ok(None);
        };
        Ok(Some(self.commit_tree(commit)?.root()))
    }

    /// `root` and every tree beneath it, except the empty tree, those
    /// `known` already holds and the trees beneath them. Only `root` must be readable: a tree
    /// beneath it that cannot be read is left out.
    pub fn trees(
        &self,
        root: ObjectId,
        known: &mut dyn FnMut(ObjectId) -> bool,
    ) -> Result<Vec<TreeRows>, GitError> {
        let mut pending = vec![root];
        let mut trees: Vec<TreeRows> = Vec::new();
        let mut pointers = Pointers::default();
        while let Some(id) = pending.pop() {
            if is_empty(id) || known(id) || trees.iter().any(|tree| tree.id == id) {
                continue;
            }
            let entries = match self.tree_entries(id, Sizes::All) {
                Ok(entries) => entries,
                Err(error) if id == root => return Err(error),
                Err(_) => continue,
            };
            let raw: Vec<_> = entries.iter().map(|(raw, _, _)| raw.clone()).collect();
            let addressed = address::directory(&LogicalPath::root(), &raw, |record| {
                pointers.shadows(record, || match record.kind {
                    Kind::File(blob) => self.read_blob(blob),
                    _ => Ok(None),
                })
            });
            let rows = entries
                .into_iter()
                .zip(addressed)
                .map(|((raw, mode, object), (logical, code))| {
                    if let Kind::Directory(child) = &raw.kind {
                        pending.push(*child);
                    }
                    TreeRow {
                        name: raw.name,
                        mode,
                        object,
                        size: raw.size,
                        segment: logical
                            .and_then(|logical| logical.segments().last().map(str::to_owned)),
                        code,
                    }
                })
                .collect();
            trees.push(TreeRows { id, rows });
        }
        Ok(trees)
    }
}

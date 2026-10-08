//! Trees as the index keeps them: each tree once, by id, with its entries
//! addressed (docs/architecture/index.md). Addressing a tree's entries
//! depends only on their names and the records beside directories, so it is
//! a fact about the tree's content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use board_model::{Code, LogicalPath, ObjectId, Source};

use crate::address::{self, Pointers};
use crate::git::{GitError, Repository};
use crate::tree::{Kind, RawEntry, Tree};
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
    /// The entry's name; one that is not UTF-8 is shown lossily.
    pub name: String,
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
/// the commit gives, without reading the repository.
struct StoredTree<'r> {
    repository: PathBuf,
    commit: ObjectId,
    root: ObjectId,
    rows: Rows<'r>,
    /// Each listed tree's rows, by tree, for addressing what was listed.
    listed: HashMap<ObjectId, Vec<TreeRow>>,
}

impl Tree for StoredTree<'_> {
    type Directory = ObjectId;

    fn repository(&self) -> &Path {
        &self.repository
    }

    fn source(&self) -> Source {
        Source::Commit(self.commit)
    }

    fn root(&self) -> ObjectId {
        self.root
    }

    fn list(&mut self, directory: &ObjectId) -> Result<Rc<[RawEntry<ObjectId>]>, GitError> {
        let Some(rows) = (self.rows)(*directory) else {
            return Err(GitError::Object {
                repository: self.repository.clone(),
                id: *directory,
                source: "the tree is not in the index".into(),
            });
        };
        let entries = rows
            .iter()
            .map(|row| RawEntry {
                name: row.name.as_bytes().to_vec(),
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
        self.listed.insert(*directory, rows);
        Ok(entries)
    }

    fn read(
        &mut self,
        _directory: &ObjectId,
        _entry: &RawEntry<ObjectId>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        Ok(None)
    }

    /// The stored segments and codes: addressing a tree depends only on it,
    /// so only the whole path, which may grow too long, is checked again.
    fn address(
        &mut self,
        directory: &ObjectId,
        parent: &LogicalPath,
        entries: &[RawEntry<ObjectId>],
    ) -> Vec<(Option<LogicalPath>, Option<Code>)> {
        let rows = self.listed.get(directory).map_or(&[][..], Vec::as_slice);
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

/// Walks the stored rows of `commit`'s tree `root` in `repository`, as
/// [`Repository::walk`] walks the commit: a tree whose rows are not known
/// is one degraded entry, as an unreadable one is.
pub fn walk_stored(
    repository: &Path,
    commit: ObjectId,
    root: ObjectId,
    rows: Rows<'_>,
) -> Result<Vec<Entry>, GitError> {
    walk_in(&mut StoredTree {
        repository: repository.to_path_buf(),
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

    /// `root` and every tree beneath it, except those `known` already holds
    /// and the trees beneath them. Only `root` must be readable: a tree
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
            if known(id) || trees.iter().any(|tree| tree.id == id) {
                continue;
            }
            let entries = match self.tree_entries(id) {
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
                        name: String::from_utf8_lossy(&raw.name).into_owned(),
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

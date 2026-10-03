//! Trees as the index keeps them: each tree once, by id, with its entries
//! addressed (docs/architecture.md, The index). Addressing a tree's entries
//! depends only on their names and the records beside directories, so it is
//! a fact about the tree's content.

use board_model::{Code, LogicalPath, ObjectId};

use crate::address;
use crate::git::{GitError, Repository};
use crate::tree::{Kind, Tree};

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
    /// The logical segment that addresses it, unless its name is malformed.
    pub segment: Option<String>,
    /// The first problem with its name among its siblings.
    pub code: Option<Code>,
}

impl TreeRow {
    pub const TREE_MODE: u32 = 0o40_000;
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
                let Kind::File(blob) = record.kind else {
                    return Some(false);
                };
                match self.read_blob(blob) {
                    Ok(Some(bytes)) => Some(!matches!(board_model::pointer(&bytes), Ok(None))),
                    Ok(None) => Some(false),
                    Err(_) => None,
                }
            });
            let rows = entries
                .into_iter()
                .zip(addressed)
                .map(|((raw, mode, object), (logical, code))| {
                    if let Kind::Directory(child) = raw.kind {
                        pending.push(child);
                    }
                    TreeRow {
                        name: String::from_utf8_lossy(&raw.name).into_owned(),
                        mode,
                        object,
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

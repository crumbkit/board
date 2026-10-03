//! Walking every entry of a source without following pointers
//! (docs/architecture.md, Boards and entries).

use board_model::{Code, LogicalPath, ObjectId, Source, Stat};

use crate::git::{GitError, Repository};
use crate::tree::{Kind, Tree};

/// One file in a tree. Directories are not entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path inside the tree, `/`-separated; a name that is not UTF-8 is
    /// shown lossily.
    pub path: String,
    /// The address, unless the path or one of its directories is malformed.
    pub logical: Option<LogicalPath>,
    /// The blob; `None` for a submodule or a file or directory that cannot be
    /// read, since derived data is keyed by blob alone.
    pub blob: Option<ObjectId>,
    pub status: Status,
    /// A working-tree file's or symlink's metadata.
    pub stat: Option<Stat>,
    /// The blob's size in bytes, when known.
    pub size: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded(Code),
}

/// A directory still to be listed, with the code it passes to everything
/// beneath it.
struct Pending<D> {
    directory: D,
    path: String,
    logical: Option<LogicalPath>,
    inherited: Option<Code>,
}

impl Repository {
    /// Every file in `source`, sorted by path. An entry's status is the first
    /// problem on its path, its directories' included. Only a root that
    /// cannot be read is an error: anything beneath it that cannot be read is
    /// one degraded entry, and the walk goes on.
    pub fn walk(&self, source: &Source) -> Result<Vec<Entry>, GitError> {
        match source {
            Source::Commit(commit) => walk_in(&mut self.commit_tree(*commit)?),
            Source::WorkingTree => self.walk_working_tree(&mut |_, _| None),
        }
    }

    /// Walks the working tree as [`Repository::walk`] does, but hashes a
    /// file only when `known`, given its `/`-separated path and metadata,
    /// does not already know its blob.
    pub fn walk_working_tree(
        &self,
        known: &mut dyn FnMut(&str, &Stat) -> Option<ObjectId>,
    ) -> Result<Vec<Entry>, GitError> {
        walk_in(&mut self.working_tree(known)?)
    }
}

pub(crate) fn walk_in<T: Tree>(tree: &mut T) -> Result<Vec<Entry>, GitError> {
    let mut pending = vec![Pending {
        directory: tree.root(),
        path: String::new(),
        logical: Some(LogicalPath::root()),
        inherited: None,
    }];
    let mut entries = Vec::new();
    while let Some(directory) = pending.pop() {
        let listed = match tree.list(&directory.directory) {
            Ok(listed) => listed,
            Err(error) if directory.path.is_empty() => return Err(error),
            Err(error) => {
                let code = directory.inherited.unwrap_or(error.code());
                entries.push(Entry {
                    path: directory.path,
                    logical: directory.logical,
                    blob: None,
                    status: Status::Degraded(code),
                    stat: None,
                    size: None,
                });
                continue;
            }
        };
        let addressed = match &directory.logical {
            Some(parent) => tree.address(&directory.directory, parent, &listed),
            None => vec![(None, None); listed.len()],
        };
        for (entry, (logical, own)) in listed.iter().zip(addressed) {
            let name = String::from_utf8_lossy(&entry.name);
            let path = if directory.path.is_empty() {
                name.into_owned()
            } else {
                format!("{}/{name}", directory.path)
            };
            let code = directory.inherited.or(own);
            let blob = match &entry.kind {
                Kind::Directory(child) => {
                    pending.push(Pending {
                        directory: child.clone(),
                        path,
                        logical,
                        inherited: code,
                    });
                    continue;
                }
                Kind::File(id) | Kind::Symlink(id) => Some(*id),
                Kind::Submodule | Kind::Unreadable => None,
            };
            entries.push(Entry {
                path,
                logical,
                blob,
                status: code.map_or(Status::Ok, Status::Degraded),
                stat: entry.stat,
                size: blob.and(entry.size),
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

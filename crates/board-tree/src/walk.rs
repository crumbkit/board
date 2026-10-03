//! Walking every entry of a source without following pointers
//! (docs/architecture.md, Boards and entries).

use std::collections::HashMap;

use board_model::{Code, LogicalPath, ObjectId, Source};

use crate::address;
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
    pub change: Change,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded(Code),
}

/// Whether an entry is what `HEAD` holds at its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Committed,
    Uncommitted,
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
    /// one degraded entry, and the walk goes on. In the working tree,
    /// an entry that differs from `HEAD` is marked uncommitted.
    pub fn walk(&self, source: &Source) -> Result<Vec<Entry>, GitError> {
        match source {
            Source::Commit(commit) => walk_in(&mut self.commit_tree(*commit)?),
            Source::WorkingTree => {
                let mut entries = walk_in(&mut self.working_tree()?)?;
                let committed: HashMap<String, Option<ObjectId>> = match self.head()? {
                    Some(head) => walk_in(&mut self.commit_tree(head)?)?
                        .into_iter()
                        .map(|entry| (entry.path, entry.blob))
                        .collect(),
                    None => HashMap::new(),
                };
                for entry in &mut entries {
                    if committed.get(&entry.path) != Some(&entry.blob) {
                        entry.change = Change::Uncommitted;
                    }
                }
                Ok(entries)
            }
        }
    }
}

fn walk_in<T: Tree>(tree: &mut T) -> Result<Vec<Entry>, GitError> {
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
                    change: Change::Committed,
                });
                continue;
            }
        };
        let addressed = match &directory.logical {
            Some(parent) => address::directory(parent, &listed, |record| {
                // A record beside its directory is read only to learn
                // whether it is a pointer, which shadows the directory.
                tree.read(&directory.directory, record)
                    .ok()
                    .flatten()
                    .is_some_and(|bytes| !matches!(board_model::pointer(&bytes), Ok(None)))
            }),
            None => vec![(None, None); listed.len()],
        };
        for (entry, (logical, own)) in listed.into_iter().zip(addressed) {
            let name = String::from_utf8_lossy(&entry.name);
            let path = if directory.path.is_empty() {
                name.into_owned()
            } else {
                format!("{}/{name}", directory.path)
            };
            let code = directory.inherited.or(own);
            let blob = match entry.kind {
                Kind::Directory(child) => {
                    pending.push(Pending {
                        directory: child,
                        path,
                        logical,
                        inherited: code,
                    });
                    continue;
                }
                Kind::File(id) | Kind::Symlink(id) => Some(id),
                Kind::Submodule | Kind::Unreadable => None,
            };
            entries.push(Entry {
                path,
                logical,
                blob,
                status: code.map_or(Status::Ok, Status::Degraded),
                change: Change::Committed,
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

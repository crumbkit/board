//! Walking every entry of a tree without following pointers
//! (docs/architecture.md, Boards and entries).

use board_model::{Code, LogicalPath, ObjectId};

use crate::address;
use crate::git::{GitError, Kind, Repository};

/// One file in a tree. Directories are not entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path inside the tree, `/`-separated; a name that is not UTF-8 is
    /// shown lossily.
    pub path: String,
    /// The address, unless the path or one of its directories is malformed.
    pub logical: Option<LogicalPath>,
    pub blob: ObjectId,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded(Code),
}

/// A directory still to be listed: its tree, physical path, logical path and
/// the code it passes to everything beneath it.
struct Pending {
    tree: ObjectId,
    path: String,
    logical: Option<LogicalPath>,
    inherited: Option<Code>,
}

impl Repository {
    /// Every file in the tree of `commit`, sorted by path. An entry's status
    /// is the first problem on its path, its directories' included.
    pub fn walk(&self, commit: ObjectId) -> Result<Vec<Entry>, GitError> {
        let mut pending = vec![Pending {
            tree: self.root_tree(commit)?,
            path: String::new(),
            logical: Some(LogicalPath::root()),
            inherited: None,
        }];
        let mut entries = Vec::new();
        while let Some(directory) = pending.pop() {
            let listed = self.list(directory.tree)?;
            let addressed = match &directory.logical {
                Some(parent) => address::directory(parent, &listed),
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
                if entry.kind == Kind::Directory {
                    pending.push(Pending {
                        tree: entry.id,
                        path,
                        logical,
                        inherited: code,
                    });
                } else {
                    entries.push(Entry {
                        path,
                        logical,
                        blob: entry.id,
                        status: code.map_or(Status::Ok, Status::Degraded),
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }
}

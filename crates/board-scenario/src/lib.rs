//! The scenario builder: real git repositories in temporary directories,
//! written with gix so that no test runs `git`. Test support only: every
//! crate uses it as a dev-dependency, and a broken fixture fails the test.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_panics_doc,
    reason = "test support: a fixture that cannot be built fails the test"
)]

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use board_model::ObjectId;
use gix::bstr::BStr;
use gix::object::tree::EntryKind;
use tempfile::TempDir;

#[cfg(test)]
mod tests;

/// What a scenario puts at one path of a commit's tree.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    /// A file last modified an hour ago, so that the index's racy-clean
    /// rule (docs/architecture/index.md) trusts its metadata.
    File(&'a str),
    /// A file modified just now, as an editor leaves it.
    Fresh(&'a str),
    /// A file whose modification time is an hour ahead, so it is newer than
    /// any refresh that has begun, however slow.
    Ahead(&'a str),
    /// A file holding any bytes, UTF-8 or not.
    Bytes(&'a [u8]),
    Symlink(&'a str),
    /// A submodule pinned to an arbitrary commit id.
    Submodule,
    /// A top-level directory whose tree object is absent from the repository.
    MissingTree,
    /// A top-level file whose blob object is absent from the repository.
    MissingBlob,
    /// Text added to the end of the file at the path, such as a section of
    /// `.git/config`.
    Appended(&'a str),
    /// Nothing: the file or directory at the path is deleted; working tree
    /// only.
    Deleted,
    /// A file the walk's user cannot read (mode 000); working tree only.
    Unreadable(&'a str),
    /// A directory, made if absent, that the walk's user cannot list (mode
    /// 000); working tree only. The scenario unlocks it when dropped, so its
    /// temporary directory can be removed.
    Locked,
    /// A directory, made if absent, that the walk's user can pass through
    /// but not list (mode 111); working tree only. Unlocked like `Locked`.
    Searchable,
    /// A file anyone may run (mode 755), such as a fixture plugin; working
    /// tree only.
    Executable(&'a str),
    /// A FIFO, whose opener waits until another process opens its other
    /// end; working tree only.
    Fifo,
}

/// One repository in its own temporary directory.
#[derive(Debug)]
pub struct Scenario {
    _directory: TempDir,
    git: gix::Repository,
    locked: RefCell<Vec<PathBuf>>,
}

impl Drop for Scenario {
    fn drop(&mut self) {
        // An inner directory locked before its parent is unlocked after it.
        for path in self.locked.take().into_iter().rev() {
            // A directory deleted since it was locked has nothing to unlock.
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
        }
    }
}

impl Default for Scenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let git = gix::init(directory.path()).unwrap();
        Self {
            _directory: directory,
            git,
            locked: RefCell::default(),
        }
    }

    /// The working directory, as a mount's `repo` names it.
    pub fn path(&self) -> String {
        self.git.workdir().unwrap().to_str().unwrap().to_owned()
    }

    /// The working directory.
    pub fn workdir(&self) -> &Path {
        self.git.workdir().unwrap()
    }

    /// Runs `during` while a writer waits to open the FIFO at the
    /// `/`-separated `path` and close it at once, so code that opens the FIFO
    /// to read it reads an empty file instead of waiting forever.
    #[expect(
        clippy::disallowed_methods,
        reason = "the scenario builder is the one writer of test files, and opens a FIFO's write end"
    )]
    pub fn feeding<T>(&self, path: &str, during: impl FnOnce() -> T) -> T {
        let path = self.git.workdir().unwrap().join(path);
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| fs::File::options().write(true).open(&path).map(drop));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(during));
            // Opening both ends does not wait on Linux or macOS, and lets a
            // writer that no reader met finish.
            let both = fs::File::options().read(true).write(true).open(&path);
            writer.join().unwrap().unwrap();
            drop(both);
            result.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    /// Writes `item` at the `/`-separated `path` in the working tree, creating
    /// its directories. Every file but a fresh one is dated an hour ago. A submodule is written as a nested repository: a
    /// directory holding a `.git` file.
    #[expect(
        clippy::disallowed_methods,
        reason = "the scenario builder is the one writer of test files, and runs mkfifo, which std cannot do safely"
    )]
    pub fn write(&self, path: &str, item: Item<'_>) -> &Self {
        let path = self.git.workdir().unwrap().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let modified = |path: &Path, time: SystemTime| {
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        };
        let backdate = |path: &Path| modified(path, SystemTime::now() - Duration::from_hours(1));
        match item {
            Item::Fresh(text) => fs::write(path, text).unwrap(),
            Item::Ahead(text) => {
                fs::write(&path, text).unwrap();
                modified(&path, SystemTime::now() + Duration::from_hours(1));
            }
            Item::File(text) => {
                fs::write(&path, text).unwrap();
                backdate(&path);
            }
            Item::Bytes(bytes) => {
                fs::write(&path, bytes).unwrap();
                backdate(&path);
            }
            Item::Symlink(target) => std::os::unix::fs::symlink(target, path).unwrap(),
            Item::Deleted if path.is_dir() => fs::remove_dir_all(path).unwrap(),
            Item::Deleted => fs::remove_file(path).unwrap(),
            Item::Appended(text) => {
                use std::io::Write;
                fs::File::options()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .unwrap()
                    .write_all(text.as_bytes())
                    .unwrap();
            }
            Item::Submodule => {
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join(".git"), "gitdir: elsewhere").unwrap();
            }
            Item::Unreadable(text) => {
                fs::write(&path, text).unwrap();
                backdate(&path);
                fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            }
            Item::Locked | Item::Searchable => {
                let mode = if matches!(item, Item::Locked) {
                    0o000
                } else {
                    0o111
                };
                fs::create_dir_all(&path).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
                self.locked.borrow_mut().push(path);
            }
            Item::Executable(text) => {
                fs::write(&path, text).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            Item::Fifo => {
                let made = std::process::Command::new("mkfifo").arg(&path).status();
                assert!(made.unwrap().success(), "mkfifo {}", path.display());
            }
            Item::MissingTree | Item::MissingBlob => {
                panic!("a working tree cannot hold a missing object")
            }
        }
        self
    }

    /// Makes the index hold the tree of `commit`, so its files are tracked.
    pub fn track(&self, commit: ObjectId) -> &Self {
        let commit = gix::ObjectId::try_from(commit.as_bytes()).unwrap();
        let tree = self.git.find_commit(commit).unwrap().tree_id().unwrap();
        let mut index = self.git.index_from_tree(&tree).unwrap();
        index.write(gix::index::write::Options::default()).unwrap();
        self
    }

    /// The id of a blob holding `text`.
    pub fn blob(&self, text: &str) -> ObjectId {
        model(self.git.write_blob(text).unwrap().detach())
    }

    /// Commits a tree of text files at `/`-separated paths.
    pub fn commit(&self, files: &[(&str, &str)]) -> ObjectId {
        let items: Vec<(&[u8], Item<'_>)> = files
            .iter()
            .map(|(path, text)| (path.as_bytes(), Item::File(text)))
            .collect();
        self.commit_items(&items)
    }

    /// Commits a tree holding each item at its path, which may be any bytes.
    pub fn commit_items(&self, items: &[(&[u8], Item<'_>)]) -> ObjectId {
        let empty = gix::ObjectId::empty_tree(self.git.object_hash());
        let mut editor = self.git.edit_tree(empty).unwrap();
        for (path, item) in items {
            let (kind, id) = match item {
                Item::File(text) | Item::Fresh(text) | Item::Ahead(text) => {
                    (EntryKind::Blob, self.git.write_blob(text).unwrap().detach())
                }
                Item::Bytes(bytes) => (
                    EntryKind::Blob,
                    self.git.write_blob(bytes).unwrap().detach(),
                ),
                Item::Symlink(target) => (
                    EntryKind::Link,
                    self.git.write_blob(target).unwrap().detach(),
                ),
                Item::MissingTree | Item::MissingBlob => continue,
                Item::Unreadable(_) => panic!("a commit cannot hold an unreadable file"),
                Item::Locked | Item::Searchable => {
                    panic!("a commit cannot hold a locked directory")
                }
                Item::Executable(_) => panic!("fixture plugins live outside commits"),
                Item::Fifo => panic!("git cannot hold a FIFO"),
                Item::Deleted | Item::Appended(_) => {
                    panic!("a commit is built from whole files")
                }
                Item::Submodule => (
                    EntryKind::Commit,
                    gix::ObjectId::from_hex(&[b'1'; 40]).unwrap(),
                ),
            };
            editor.upsert(BStr::new(path), kind, id).unwrap();
        }
        let mut tree = editor.write().unwrap().detach();
        // The editor refuses an object it cannot find, so absent ones are
        // added to the root tree object directly.
        let missing: Vec<(&[u8], EntryKind, u8)> = items
            .iter()
            .filter_map(|(path, item)| match item {
                Item::MissingTree => Some((*path, EntryKind::Tree, b'2')),
                Item::MissingBlob => Some((*path, EntryKind::Blob, b'3')),
                _ => None,
            })
            .collect();
        if !missing.is_empty() {
            let root = self.git.find_tree(tree).unwrap();
            let mut root: gix::objs::Tree = root.decode().unwrap().into();
            for (name, kind, digit) in missing {
                root.entries.push(gix::objs::tree::Entry {
                    mode: kind.into(),
                    filename: name.into(),
                    oid: gix::ObjectId::from_hex(&[digit; 40]).unwrap(),
                });
            }
            root.entries.sort();
            tree = self.git.write_object(&root).unwrap().detach();
        }
        let signature = gix::actor::SignatureRef {
            name: "scenario".into(),
            email: "scenario@example.com".into(),
            time: "0 +0000",
        };
        let parents: Vec<gix::ObjectId> = self
            .git
            .head_id()
            .ok()
            .map(gix::Id::detach)
            .into_iter()
            .collect();
        let commit = self
            .git
            .commit_as(signature, signature, "HEAD", "scenario", tree, parents)
            .unwrap();
        model(commit.detach())
    }
}

/// The commit HEAD names, as a test reads it.
#[derive(Debug)]
pub struct HeadCommit {
    pub id: ObjectId,
    pub message: String,
    pub parents: Vec<ObjectId>,
    /// Every file in its tree, by `/`-separated path.
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
}

impl Scenario {
    /// The commit HEAD names now, read afresh, or `None` before the first.
    pub fn head(&self) -> Option<HeadCommit> {
        let git = gix::open(self.workdir()).unwrap();
        let id = git.head_id().ok()?.detach();
        let commit = git.find_commit(id).unwrap();
        let mut files = std::collections::BTreeMap::new();
        let mut pending = vec![(String::new(), commit.tree_id().unwrap().detach())];
        while let Some((prefix, tree)) = pending.pop() {
            for entry in git.find_tree(tree).unwrap().decode().unwrap().entries {
                let path = format!("{prefix}{}", entry.filename);
                if entry.mode.is_tree() {
                    pending.push((format!("{path}/"), entry.oid.to_owned()));
                } else {
                    files.insert(path, git.find_blob(entry.oid).unwrap().data.clone());
                }
            }
        }
        Some(HeadCommit {
            id: model(id),
            message: commit.message_raw_sloppy().to_string(),
            parents: commit
                .parent_ids()
                .map(|parent| model(parent.detach()))
                .collect(),
            files,
        })
    }

    /// The paths git's index holds, each with its blob.
    pub fn index(&self) -> std::collections::BTreeMap<String, ObjectId> {
        let git = gix::open(self.workdir()).unwrap();
        let index = git.index_or_empty().unwrap();
        index
            .entries()
            .iter()
            .map(|entry| (entry.path(&index).to_string(), model(entry.id)))
            .collect()
    }
}

fn model(id: gix::ObjectId) -> ObjectId {
    ObjectId::Sha1(id.as_bytes().try_into().unwrap())
}

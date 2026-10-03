//! The scenario builder: real git repositories in temporary directories,
//! written with gix so that no test runs `git`. Test support only: every
//! crate uses it as a dev-dependency, and a broken fixture fails the test.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_panics_doc,
    reason = "test support: a fixture that cannot be built fails the test"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use board_model::ObjectId;
use gix::bstr::BStr;
use gix::object::tree::EntryKind;
use tempfile::TempDir;

/// What a scenario puts at one path of a commit's tree.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    File(&'a str),
    /// A file holding any bytes, UTF-8 or not.
    Bytes(&'a [u8]),
    Symlink(&'a str),
    /// A submodule pinned to an arbitrary commit id.
    Submodule,
    /// A top-level directory whose tree object is absent from the repository.
    MissingTree,
    /// A top-level file whose blob object is absent from the repository.
    MissingBlob,
    /// A file the walk's user cannot read (mode 000); working tree only.
    Unreadable(&'a str),
}

/// One repository in its own temporary directory.
#[derive(Debug)]
pub struct Scenario {
    _directory: TempDir,
    git: gix::Repository,
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

    /// Writes `item` at the `/`-separated `path` in the working tree, creating
    /// its directories. A submodule is written as a nested repository: a
    /// directory holding a `.git` file.
    #[expect(
        clippy::disallowed_methods,
        reason = "the scenario builder is the one writer of test files"
    )]
    pub fn write(&self, path: &str, item: Item<'_>) -> &Self {
        let path = self.git.workdir().unwrap().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match item {
            Item::File(text) => fs::write(path, text).unwrap(),
            Item::Bytes(bytes) => fs::write(path, bytes).unwrap(),
            Item::Symlink(target) => std::os::unix::fs::symlink(target, path).unwrap(),
            Item::Submodule => {
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join(".git"), "gitdir: elsewhere").unwrap();
            }
            Item::Unreadable(text) => {
                fs::write(&path, text).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
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
                Item::File(text) => (EntryKind::Blob, self.git.write_blob(text).unwrap().detach()),
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

fn model(id: gix::ObjectId) -> ObjectId {
    ObjectId::Sha1(id.as_bytes().try_into().unwrap())
}

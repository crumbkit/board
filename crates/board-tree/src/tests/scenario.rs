//! The scenario builder: real git repositories in temporary directories,
//! written with gix so that no test runs `git`.

use std::fs;

use board_model::ObjectId;
use gix::bstr::BStr;
use gix::object::tree::EntryKind;
use tempfile::TempDir;

use crate::git::Repository;

/// What a scenario puts at one path of a commit's tree.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Item<'a> {
    File(&'a str),
    Symlink(&'a str),
    /// A submodule pinned to an arbitrary commit id.
    Submodule,
}

/// One repository in its own temporary directory.
pub(crate) struct Scenario {
    _directory: TempDir,
    git: gix::Repository,
}

impl Scenario {
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let git = gix::init(directory.path()).unwrap();
        Self {
            _directory: directory,
            git,
        }
    }

    pub(crate) fn open(&self) -> Repository {
        Repository::open(self.git.workdir().unwrap()).unwrap()
    }

    /// Writes `item` at the `/`-separated `path` in the working tree, creating
    /// its directories. A submodule is written as a nested repository: a
    /// directory holding a `.git` file.
    #[expect(
        clippy::disallowed_methods,
        reason = "the scenario builder is the one writer of test files"
    )]
    pub(crate) fn write(&self, path: &str, item: Item<'_>) -> &Self {
        let path = self.git.workdir().unwrap().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match item {
            Item::File(text) => fs::write(path, text).unwrap(),
            Item::Symlink(target) => std::os::unix::fs::symlink(target, path).unwrap(),
            Item::Submodule => {
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join(".git"), "gitdir: elsewhere").unwrap();
            }
        }
        self
    }

    /// Makes the index hold the tree of `commit`, so its files are tracked.
    pub(crate) fn track(&self, commit: ObjectId) -> &Self {
        let commit = gix::ObjectId::try_from(commit.as_bytes()).unwrap();
        let tree = self.git.find_commit(commit).unwrap().tree_id().unwrap();
        let mut index = self.git.index_from_tree(&tree).unwrap();
        index.write(gix::index::write::Options::default()).unwrap();
        self
    }

    /// The id of a blob holding `text`.
    pub(crate) fn blob(&self, text: &str) -> ObjectId {
        model(self.git.write_blob(text).unwrap().detach())
    }

    /// Commits a tree of text files at `/`-separated paths.
    pub(crate) fn commit(&self, files: &[(&str, &str)]) -> ObjectId {
        let items: Vec<(&[u8], Item<'_>)> = files
            .iter()
            .map(|(path, text)| (path.as_bytes(), Item::File(text)))
            .collect();
        self.commit_items(&items)
    }

    /// Commits a tree holding each item at its path, which may be any bytes.
    pub(crate) fn commit_items(&self, items: &[(&[u8], Item<'_>)]) -> ObjectId {
        let empty = gix::ObjectId::empty_tree(self.git.object_hash());
        let mut editor = self.git.edit_tree(empty).unwrap();
        for (path, item) in items {
            let (kind, id) = match item {
                Item::File(text) => (EntryKind::Blob, self.git.write_blob(text).unwrap().detach()),
                Item::Symlink(target) => (
                    EntryKind::Link,
                    self.git.write_blob(target).unwrap().detach(),
                ),
                Item::Submodule => (
                    EntryKind::Commit,
                    gix::ObjectId::from_hex(&[b'1'; 40]).unwrap(),
                ),
            };
            editor.upsert(BStr::new(path), kind, id).unwrap();
        }
        let tree = editor.write().unwrap().detach();
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

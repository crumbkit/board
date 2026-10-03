//! Walking one directory's subtree, and only what leads to it.

use std::rc::Rc;

use board_model::{Code, LogicalPath, Source};

use super::*;
use crate::git::GitError;
use crate::tree::{RawEntry, Tree};
use crate::walk::walk_from;

/// A tree that counts the entries its directories list.
struct Counting<'t, T> {
    tree: &'t mut T,
    listed: usize,
}

impl<T: Tree> Tree for Counting<'_, T> {
    type Directory = T::Directory;

    fn source(&self) -> Source {
        self.tree.source()
    }

    fn root(&self) -> Self::Directory {
        self.tree.root()
    }

    fn list(
        &mut self,
        directory: &Self::Directory,
    ) -> Result<Rc<[RawEntry<Self::Directory>]>, GitError> {
        let listed = self.tree.list(directory)?;
        self.listed += listed.len();
        Ok(listed)
    }

    fn read(
        &mut self,
        directory: &Self::Directory,
        entry: &RawEntry<Self::Directory>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        self.tree.read(directory, entry)
    }
}

/// `tasks/` with three files, beside `others` files elsewhere.
fn files(others: usize) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("tasks/{name}.json"), "{}".to_owned()))
        .collect();
    files.extend((0..others).map(|n| (format!("other/d{}/f{n}.json", n % 10), "{}".to_owned())));
    files
}

/// A board whose working tree and one commit both hold [`files`].
fn board_with(others: usize) -> (Scenario, Source) {
    let scenario = Scenario::new();
    let files = files(others);
    for (path, text) in &files {
        scenario.write(path, Item::File(text));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let commit = scenario.commit(&files);
    (scenario, Source::Commit(commit))
}

/// How many entries walking `tasks` lists, and how many files it yields.
fn walked(scenario: &Scenario, source: &Source) -> (usize, usize) {
    let repository = scenario.open();
    match source {
        Source::WorkingTree => {
            let mut unknown = |_: &str, _: &board_model::Stat| None;
            let mut tree = repository.working_tree(&mut unknown).unwrap();
            let mut counting = Counting {
                tree: &mut tree,
                listed: 0,
            };
            let entries = walk_from(&mut counting, "tasks").unwrap();
            (counting.listed, entries.len())
        }
        Source::Commit(commit) => {
            let mut tree = repository.commit_tree(*commit).unwrap();
            let mut counting = Counting {
                tree: &mut tree,
                listed: 0,
            };
            let entries = walk_from(&mut counting, "tasks").unwrap();
            (counting.listed, entries.len())
        }
    }
}

#[test]
fn walking_one_directory_lists_the_same_however_large_the_board() {
    let (small, small_commit) = board_with(10);
    let (large, large_commit) = board_with(1_000);
    // The root's two directories, then `tasks`' three files.
    assert_eq!(walked(&small, &Source::WorkingTree), (5, 3));
    assert_eq!(walked(&large, &Source::WorkingTree), (5, 3));
    assert_eq!(walked(&small, &small_commit), (5, 3));
    assert_eq!(walked(&large, &large_commit), (5, 3));
}

/// Asserts that walking each of `scopes` in `source` gives exactly the whole
/// walk's entries beneath it.
fn scoped_equals_whole(repository: &Repository, source: &Source, scopes: &[&str]) {
    for scope in scopes {
        let prefix = if scope.is_empty() {
            String::new()
        } else {
            format!("{scope}/")
        };
        let expected: Vec<Entry> = repository
            .walk(source)
            .unwrap()
            .into_iter()
            .filter(|entry| entry.path.starts_with(&prefix))
            .collect();
        assert_eq!(
            repository.walk_under(source, scope).unwrap(),
            expected,
            "{scope}"
        );
    }
}

#[test]
fn walking_a_directory_gives_exactly_what_the_whole_walk_gives_beneath_it() {
    // Names that differ only by case are built as a commit: a case-insensitive
    // working tree, as on macOS, cannot hold both.
    let scenario = Scenario::new();
    let commit = scenario.commit(&[
        ("work/t.json", "{}"),
        ("work/sub/x.md", ""),
        ("work/Dup.json", "{}"),
        ("work/dup.json", "{}"),
        ("Case/x.json", "{}"),
        ("case/inner/y.json", "{}"),
        ("elsewhere/z.json", "{}"),
    ]);
    let repository = scenario.open();
    let source = Source::Commit(commit);
    scoped_equals_whole(
        &repository,
        &source,
        &["work", "work/sub", "case/inner", ""],
    );
    // An ancestor's code reaches what lies beneath it.
    let inner = repository.walk_under(&source, "case/inner").unwrap();
    assert_eq!(inner[0].status, Status::Degraded(Code::PathCaseCollision));
    assert_eq!(
        inner[0].logical,
        Some("/case/inner/y".parse::<LogicalPath>().unwrap())
    );

    let scenario = Scenario::new();
    scenario
        .write("work/t.json", Item::File("{}"))
        .write("work/sub/x.md", Item::File(""))
        // A file and a record of one name shadow each other on any file system.
        .write("work/x", Item::File(""))
        .write("work/x.json", Item::File("{}"))
        .write("elsewhere/z.json", Item::File("{}"));
    let repository = scenario.open();
    scoped_equals_whole(&repository, &Source::WorkingTree, &["work", "work/sub", ""]);
    let work = repository.walk_under(&Source::WorkingTree, "work").unwrap();
    assert!(
        work.iter()
            .any(|entry| entry.status == Status::Degraded(Code::PathShadowed))
    );
}

#[test]
fn walking_a_path_that_is_not_a_directory_finds_nothing() {
    let (scenario, _) = board_with(0);
    let repository = scenario.open();
    assert!(
        repository
            .walk_under(&Source::WorkingTree, "tasks/a.json")
            .unwrap()
            .is_empty()
    );
    assert!(
        repository
            .walk_under(&Source::WorkingTree, "nowhere")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repository
            .walk_under(&Source::WorkingTree, "tasks")
            .unwrap()
            .len(),
        3
    );
}

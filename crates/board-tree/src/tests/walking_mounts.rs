//! Walking every board reachable through mounts with a cache that keeps
//! each source's list of pointers and its decision.

use std::collections::HashMap;

use board_model::Digest;

use super::*;
use crate::{Cache, GitError, Held, Reached, TreeRow, TreeRows};

/// The facts of a source: the physical paths that hold a pointer.
type Facts = HashSet<String>;

/// A cache in memory. Every mounted source holds no pointer, and every
/// decision is kept or none is.
#[derive(Default)]
struct Memory {
    trees: HashMap<ObjectId, Vec<TreeRow>>,
    pointers: HashMap<Digest, Vec<Held>>,
    kept: bool,
}

impl Cache for Memory {
    type Error = GitError;
    type Facts = Facts;

    fn has_tree(&mut self, tree: ObjectId) -> bool {
        self.trees.contains_key(&tree)
    }

    fn put_tree(&mut self, tree: &TreeRows) -> Result<(), GitError> {
        self.trees.insert(tree.id, tree.rows.clone());
        Ok(())
    }

    fn rows(&mut self, tree: ObjectId) -> Option<Vec<TreeRow>> {
        self.trees.get(&tree).cloned()
    }

    fn facts(&mut self, _: &Repository, _: &Source, _: &[Entry]) -> Result<Facts, GitError> {
        Ok(Facts::new())
    }

    fn is_pointer(facts: &Facts, path: &str) -> bool {
        facts.contains(path)
    }

    fn complete(_: &Facts, _: &[Entry]) -> bool {
        true
    }

    fn pointers(&mut self, scan: &Digest) -> Option<Vec<Held>> {
        self.pointers.get(scan).cloned()
    }

    fn put_pointers(&mut self, scan: &Digest, held: &[Held]) -> Result<(), GitError> {
        self.pointers.insert(*scan, held.to_vec());
        Ok(())
    }

    fn kept(&mut self, _: &Digest) -> bool {
        self.kept
    }
}

/// A board whose working tree mounts, at `/m`, a library that holds `p.json`.
fn mounting() -> (Scenario, Scenario) {
    let library = Scenario::new();
    let commit = library.commit(&[("p.json", "{}")]);
    let board = Scenario::new();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
        library.path()
    );
    board.write("m.json", Item::File(&mount));
    (board, library)
}

/// Walks the mounts of `board`'s working tree as the build `test`.
fn walk_mounts(board: &Scenario, cache: &mut Memory, skip_kept: bool) -> Reached<Facts> {
    let entries = board.open().walk(&Source::WorkingTree).unwrap();
    let facts = Facts::from(["m.json".to_owned()]);
    (board.repositories())
        .walk_mounts(&entries, &facts, cache, ("test", skip_kept), None)
        .unwrap()
}

#[test]
fn a_walk_that_skips_kept_sources_walks_a_listed_source_only_when_its_decision_is_not_kept() {
    let (board, _library) = mounting();
    let mut cache = Memory::default();
    let first = walk_mounts(&board, &mut cache, true);
    assert!(
        first.sources[0].walked.is_some(),
        "an unlisted source is walked"
    );
    for (kept, walked) in [(true, false), (false, true)] {
        cache.kept = kept;
        let reached = walk_mounts(&board, &mut cache, true);
        assert_eq!(reached.sources[0].kept, kept);
        assert_eq!(reached.sources[0].walked.is_some(), walked, "kept: {kept}");
    }
}

#[test]
fn a_source_whose_listed_pointer_cannot_be_read_has_no_key_and_its_readable_twin_has_one() {
    let (board, library) = mounting();
    let mut cache = Memory::default();
    let scan = walk_mounts(&board, &mut cache, false).sources[0].scan;
    // A blob of the board's own repository, which the library lacks.
    let missing = board.blob("only in the board");
    for (blob, keyed) in [(missing, false), (library.blob("{}"), true)] {
        let listed = Held {
            path: "p.json".to_owned(),
            logical: path("/p"),
            blob,
        };
        cache.pointers.insert(scan, vec![listed]);
        let reached = walk_mounts(&board, &mut cache, false);
        assert_eq!(reached.sources[0].key.is_some(), keyed, "{blob}");
    }
}

//! Walking every board reachable through mounts with a cache that keeps
//! each source's list of pointers and its decision.

use std::collections::HashMap;

use board_model::Digest;

use super::*;
use crate::{Cache, GitError, Held, Reached, TreeRow, TreeRows};

/// The facts of a source: the physical paths that hold a pointer.
type Facts = HashSet<String>;

/// A cache in memory. A mounted source holds a pointer at each physical
/// path in `pointing`, and every decision is kept or none is.
#[derive(Default)]
struct Memory {
    trees: HashMap<ObjectId, Vec<TreeRow>>,
    pointers: HashMap<Digest, Vec<Held>>,
    kept: bool,
    pointing: Facts,
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

    fn facts(&mut self, _: &Repository, _: &Source, entries: &[Entry]) -> Result<Facts, GitError> {
        let pointing = |entry: &&Entry| self.pointing.contains(&entry.path);
        Ok(entries
            .iter()
            .filter(pointing)
            .map(|entry| entry.path.clone())
            .collect())
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

#[test]
fn a_walk_reaches_a_mount_read_at_a_checkout_at_that_commit_and_its_twin_at_its_pin() {
    let (board, library) = mounting();
    let pinned = library.head().unwrap().id;
    let head = library.commit(&[("p.json", "{}"), ("q.json", "{}")]);
    let entries = board.open().walk(&Source::WorkingTree).unwrap();
    let facts = Facts::from(["m.json".to_owned()]);
    for (checkout, reached) in [(Some(head), head), (None, pinned)] {
        let mut repositories = board.repositories();
        if let Some(head) = checkout {
            repositories.read_at(crate::Checkout {
                pointer: "m.json".to_owned(),
                commit: head,
            });
        }
        let walked = (repositories.walk_mounts(
            &entries,
            &facts,
            &mut Memory::default(),
            ("test", false),
            None,
        ))
        .unwrap();
        let commits: Vec<ObjectId> = walked
            .sources
            .iter()
            .map(|mounted| mounted.commit)
            .collect();
        assert_eq!(commits, [reached], "{checkout:?}");
        assert_eq!(walked.points[0].target, Ok(reached));
    }
}

/// A mount of the repository at `repo` at `commit`.
fn mount_of(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A linked worktree of `main`, as `git worktree add --detach` lays it out,
/// at `linked` in another repository's working tree, checked out at `commit`.
/// It returns that repository, which must outlive the worktree, and the
/// worktree's path.
fn linked_worktree(main: &Scenario, commit: ObjectId) -> (Scenario, String) {
    let host = Scenario::new();
    let linked = format!("{}/linked", host.path());
    let git_dir = format!("{}/.git/worktrees/linked", main.path());
    main.write(
        ".git/worktrees/linked/HEAD",
        Item::File(&format!("{commit}\n")),
    )
    .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
    .write(
        ".git/worktrees/linked/gitdir",
        Item::File(&format!("{linked}/.git\n")),
    );
    host.write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")));
    (host, linked)
}

/// `scenario` with `url` as its `origin`.
fn originating(scenario: &Scenario, url: &str) {
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    scenario.write(".git/config", Item::Appended(&remote));
}

/// A board R that mounts B at `/b` and C at `/c`, where C's one commit holds
/// `peer.json`, the pointer `peer` gives the commit B was mounted at.
struct Triangle {
    board: Scenario,
    b: (Scenario, ObjectId),
    c: (Scenario, ObjectId),
}

impl Triangle {
    fn new(peer: impl FnOnce(&Scenario, &Scenario, ObjectId) -> String) -> Self {
        let board = Scenario::new();
        let b = Scenario::new();
        let b_commit = b.commit(&[("b.json", "{}")]);
        let c = Scenario::new();
        let pointer = peer(&board, &b, b_commit);
        let c_commit = c.commit(&[("c.json", "{}"), ("peer.json", &pointer)]);
        board
            .write("r.json", Item::File("{}"))
            .write("b.json", Item::File(&mount_of(&b.path(), b_commit)))
            .write("c.json", Item::File(&mount_of(&c.path(), c_commit)));
        Self {
            board,
            b: (b, b_commit),
            c: (c, c_commit),
        }
    }

    /// The walk, and how many repositories it opened from disk.
    fn walk(&self) -> (Reached<Facts>, usize) {
        let mut cache = Memory {
            pointing: Facts::from(["peer.json".to_owned()]),
            ..Memory::default()
        };
        let entries = self.board.open().walk(&Source::WorkingTree).unwrap();
        let facts = Facts::from(["b.json".to_owned(), "c.json".to_owned()]);
        let repositories = self.board.repositories();
        let reached = repositories
            .walk_mounts(&entries, &facts, &mut cache, ("test", false), None)
            .unwrap();
        (reached, repositories.opened())
    }
}

/// Each walked source's commit, by route.
fn routes(reached: &Reached<Facts>) -> Vec<(String, ObjectId)> {
    let mut routes: Vec<(String, ObjectId)> = (reached.sources.iter())
        .map(|mounted| (mounted.route.to_string(), mounted.commit))
        .collect();
    routes.sort();
    routes
}

/// What the pointer `peer.json` in the commit `c` reaches.
fn peer(reached: &Reached<Facts>, c: ObjectId) -> Result<ObjectId, Code> {
    let at =
        |point: &&crate::MountPoint| point.source == Source::Commit(c) && point.path == "peer.json";
    reached.points.iter().find(at).unwrap().target
}

#[test]
fn a_mount_of_a_linked_worktree_of_the_root_is_a_cycle_and_a_second_mount_of_a_sibling_is_walked() {
    let mut _host = None;
    let refused = Triangle::new(|board, _, _| {
        let r_commit = board.commit(&[("r.json", "{}")]);
        let (host, worktree) = linked_worktree(board, r_commit);
        _host = Some(host);
        mount_of(&worktree, r_commit)
    });
    let (reached, opened) = refused.walk();
    let (b, c) = (refused.b.1, refused.c.1);
    assert_eq!(
        routes(&reached),
        [("/b".to_owned(), b), ("/c".to_owned(), c)],
        "the root's records appear once, from its working tree"
    );
    assert_eq!(peer(&reached, c), Err(Code::MountCycle));
    assert_eq!(opened, 3, "B, C and the linked worktree, each once");

    let admitted = Triangle::new(|_, b, _| mount_of(&b.path(), b.commit(&[("b2.json", "{}")])));
    let (reached, opened) = admitted.walk();
    let (b, c) = (admitted.b.1, admitted.c.1);
    let again = peer(&reached, c).unwrap();
    assert_ne!(again, b);
    assert_eq!(
        routes(&reached),
        [
            ("/b".to_owned(), b),
            ("/c".to_owned(), c),
            ("/c/peer".to_owned(), again)
        ],
        "B beside C is not above it"
    );
    assert_eq!(opened, 2, "B and C, each once");
}

#[test]
fn a_mount_of_a_clone_of_the_root_with_its_origin_is_a_cycle_and_one_with_another_origin_is_walked()
{
    let origin = "https://github.com/example-org/r.git";
    for (url, cycle) in [
        ("git@github.com:example-org/r", true),
        ("https://github.com/example-org/other.git", false),
    ] {
        let mut _clone = None;
        let triangle = Triangle::new(|board, _, _| {
            originating(board, origin);
            let clone = Scenario::new();
            originating(&clone, url);
            let commit = clone.commit(&[("r.json", "{}")]);
            let pointer = mount_of(&clone.path(), commit);
            _clone = Some(clone);
            pointer
        });
        let (reached, _) = triangle.walk();
        let c = triangle.c.1;
        assert_eq!(
            peer(&reached, c).is_err_and(|code| code == Code::MountCycle),
            cycle,
            "{url}"
        );
        assert_eq!(reached.sources.len(), if cycle { 2 } else { 3 }, "{url}");
    }
}

#[test]
fn two_boards_that_mount_each_other_at_successive_commits_are_each_walked_once() {
    let (a, b) = (Scenario::new(), Scenario::new());
    let b1 = b.commit(&[("b.json", "{}")]);
    let a1 = a.commit(&[("a.json", "{}"), ("peer.json", &mount_of(&b.path(), b1))]);
    let b2 = b.commit(&[("b.json", "{}"), ("peer.json", &mount_of(&a.path(), a1))]);
    a.write("a.json", Item::File("{}"))
        .write("peer.json", Item::File(&mount_of(&b.path(), b2)));
    let mut cache = Memory {
        pointing: Facts::from(["peer.json".to_owned()]),
        ..Memory::default()
    };
    let entries = a.open().walk(&Source::WorkingTree).unwrap();
    let facts = Facts::from(["peer.json".to_owned()]);
    let reached = (a.repositories())
        .walk_mounts(&entries, &facts, &mut cache, ("test", false), None)
        .unwrap();
    assert_eq!(routes(&reached), [("/peer".to_owned(), b2)]);
    assert_eq!(peer(&reached, b2), Err(Code::MountCycle));
}

#[test]
fn repositories_whose_relative_origins_name_different_directories_are_two_and_the_same_one_is_one()
{
    let (first, second) = (Scenario::new(), Scenario::new());
    let (a, b, c) = (
        Scenario::within(&first),
        Scenario::within(&first),
        Scenario::within(&second),
    );
    for scenario in [&a, &b, &c] {
        originating(scenario, "../upstream");
    }
    let identity = |scenario: &Scenario| scenario.open().identity();
    assert!(identity(&a).same(&identity(&b)), "both name first/upstream");
    assert!(!identity(&a).same(&identity(&c)), "c names second/upstream");
}

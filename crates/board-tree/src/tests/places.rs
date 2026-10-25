//! Places: which mount in a board reads the repository holding a directory,
//! and where the directory is beneath it (docs/architecture.md, Boards and
//! entries).

use std::path::Path;

use super::*;
use crate::Place;

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A board whose working tree mounts each of `mounts`, a path and a
/// repository, at that repository's one commit.
fn board(mounts: &[(&str, &Scenario)]) -> Scenario {
    let board = Scenario::new();
    for (at, repository) in mounts {
        let commit = repository.commit(&[("x.json", "1")]);
        board.write(at, Item::File(&mount(&repository.path(), commit)));
    }
    board
}

/// The places `board` gives `directory`, reading every working-tree file
/// as a possible pointer.
fn places(board: &Scenario, directory: &Path) -> Vec<Place> {
    let entries = board.open().walk(&Source::WorkingTree).unwrap();
    board.repositories().places(&entries, &|_| true, directory)
}

fn place(mount: &str, within: &str) -> Place {
    Place {
        mount: path(mount),
        within: path(within),
    }
}

#[test]
fn the_root_of_a_mounted_repository_is_placed_at_its_mount_and_an_unmounted_one_nowhere() {
    let (mounted, unmounted) = (Scenario::new(), Scenario::new());
    let board = board(&[("repos/lib.json", &mounted)]);
    let found = places(&board, mounted.workdir());
    assert_eq!(found, [place("/repos/lib", "/")]);
    assert_eq!(found[0].prefix(), "/repos/lib");
    assert_eq!(places(&board, unmounted.workdir()), []);
}

#[test]
fn a_directory_is_within_its_repository_at_its_path_and_the_git_directory_is_within_none() {
    let repository = Scenario::new();
    repository.write("docs/deep/a.md", Item::File("a"));
    let deep = repository.workdir().join("docs/deep");
    assert_eq!(crate::within(&deep), Some(Ok(path("/docs/deep"))));
    assert_eq!(crate::within(repository.workdir()), Some(Ok(path("/"))));
    assert_eq!(crate::within(&repository.workdir().join(".git")), None);
}

/// The reason `within` gives for `name`, a directory at the root of a
/// repository, or `None` when it gives a path.
#[expect(
    clippy::disallowed_methods,
    reason = "a test makes a directory whose name may not be UTF-8"
)]
fn refused(name: &std::ffi::OsStr) -> Option<board_model::Malformation> {
    let repository = Scenario::new();
    let directory = repository.workdir().join(name);
    std::fs::create_dir(&directory).unwrap();
    match crate::within(&directory) {
        Some(Err(board_model::PathError::Malformed { reason, .. })) => Some(reason),
        Some(Ok(_)) => None,
        None => panic!("{} is in no working tree", directory.display()),
    }
}

#[test]
fn a_directory_whose_name_holds_a_control_character_is_within_a_refused_path() {
    let reason = board_model::Malformation::HiddenCharacter { char: '\t' };
    assert_eq!(refused("a\tb".as_ref()), Some(reason));
    assert_eq!(refused("ab".as_ref()), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_directory_whose_name_is_not_utf8_is_within_a_refused_path() {
    use std::os::unix::ffi::OsStrExt as _;
    let reason = board_model::Malformation::NotUtf8;
    assert_eq!(
        refused(std::ffi::OsStr::from_bytes(b"a\xffb")),
        Some(reason)
    );
    assert_eq!(refused(std::ffi::OsStr::from_bytes(b"ab")), None);
}

#[test]
fn a_subdirectory_is_placed_beneath_its_mount() {
    let mounted = Scenario::new();
    mounted.write("docs/deep/a.md", Item::File("a"));
    let board = board(&[("repos/lib.json", &mounted)]);
    let found = places(&board, &mounted.workdir().join("docs/deep"));
    assert_eq!(found, [place("/repos/lib", "/docs/deep")]);
    assert_eq!(found[0].prefix(), "/repos/lib/docs/deep");
}

#[test]
fn a_linked_worktree_of_a_mounted_repository_is_placed_and_a_separate_clone_is_not() {
    let mounted = Scenario::new();
    let board = board(&[("repos/lib.json", &mounted)]);
    let pinned = mounted.commit(&[("x.json", "1")]);
    // A linked worktree as `git worktree add --detach` lays it out: its
    // `.git` is a file naming a git directory inside the main one.
    let linked = format!("{}/linked", mounted.path());
    let git_dir = format!("{}/.git/worktrees/linked", mounted.path());
    mounted
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{pinned}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write("linked/docs/a.md", Item::File("a"));
    let found = places(&board, &Path::new(&linked).join("docs"));
    assert_eq!(found, [place("/repos/lib", "/docs")]);
    // A repository with the same files but its own git directory is another
    // repository.
    let clone = Scenario::new();
    clone.commit(&[("x.json", "1")]);
    assert_eq!(places(&board, clone.workdir()), []);
}

#[test]
fn a_repository_mounted_twice_is_placed_at_each_mount() {
    let mounted = Scenario::new();
    let board = board(&[("a.json", &mounted), ("b/c.json", &mounted)]);
    assert_eq!(
        places(&board, mounted.workdir()),
        [place("/a", "/"), place("/b/c", "/")]
    );
}

#[test]
fn a_directory_in_no_repository_is_placed_nowhere_and_one_in_a_mounted_repository_is_placed() {
    let mounted = Scenario::new();
    let board = board(&[("lib.json", &mounted)]);
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(places(&board, outside.path()), []);
    assert_eq!(places(&board, mounted.workdir()), [place("/lib", "/")]);
}

#[test]
fn a_directory_inside_the_git_directory_is_placed_nowhere_and_the_working_tree_is_placed() {
    let mounted = Scenario::new();
    let board = board(&[("lib.json", &mounted)]);
    let git = mounted.workdir().join(".git");
    assert_eq!(places(&board, &git), []);
    assert_eq!(places(&board, &git.join("refs")), []);
    assert_eq!(places(&board, mounted.workdir()), [place("/lib", "/")]);
}

#[test]
fn only_entries_the_cache_calls_pointers_are_read_as_mounts() {
    let mounted = Scenario::new();
    let board = board(&[("lib.json", &mounted)]);
    let entries = board.open().walk(&Source::WorkingTree).unwrap();
    let repositories = board.repositories();
    let none = repositories.places(&entries, &|_| false, mounted.workdir());
    assert_eq!(none, []);
    let only = repositories.places(&entries, &|path| path == "lib.json", mounted.workdir());
    assert_eq!(only, [place("/lib", "/")]);
}

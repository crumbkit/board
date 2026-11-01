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
    let every = |_: &str| true;
    board
        .repositories()
        .places(&entries, &every, &every, directory)
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
    assert_eq!(found[0].prefix(), Ok(path("/repos/lib")));
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
    assert_eq!(found[0].prefix(), Ok(path("/repos/lib/docs/deep")));
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
    let (no, lib) = (|_: &str| false, |path: &str| path == "lib.json");
    let none = repositories.places(&entries, &no, &no, mounted.workdir());
    assert_eq!(none, []);
    let only = repositories.places(&entries, &lib, &no, mounted.workdir());
    assert_eq!(only, [place("/lib", "/")]);
}

/// `scenario` with `url` as its `origin`.
fn originating<'s>(scenario: &'s Scenario, url: &str) -> &'s Scenario {
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    scenario.write(".git/config", Item::Appended(&remote))
}

fn repo(remote: &str) -> String {
    format!(r#"{{"$type": "board/repo/v1", "remote": "{remote}"}}"#)
}

#[test]
fn an_unmounted_clone_is_placed_at_the_repo_record_its_origin_names_and_one_named_by_none_nowhere()
{
    let (clone, other) = (Scenario::new(), Scenario::new());
    originating(&clone, "git@github.com-work:example-org/lib.git");
    originating(&other, "https://github.com/example-org/other.git");
    clone.write("docs/a.md", Item::File("a"));
    let board = Scenario::new();
    board.write("repos/lib.json", Item::File(&repo("example-org/lib")));
    assert_eq!(places(&board, clone.workdir()), [place("/repos/lib", "/")]);
    let docs = places(&board, &clone.workdir().join("docs"));
    assert_eq!(docs, [place("/repos/lib", "/")]);
    assert_eq!(places(&board, other.workdir()), []);
}

#[test]
fn a_clone_with_no_origin_is_placed_nowhere_and_its_twin_with_one_at_its_record() {
    let (bare, originated) = (Scenario::new(), Scenario::new());
    originating(&originated, "https://github.com/example-org/lib");
    let board = Scenario::new();
    board.write("repos/lib.json", Item::File(&repo("example-org/lib")));
    assert_eq!(places(&board, bare.workdir()), []);
    assert_eq!(
        places(&board, originated.workdir()),
        [place("/repos/lib", "/")]
    );
}

#[test]
fn a_second_clone_of_a_mounted_repository_is_placed_at_its_repo_record_by_its_origin() {
    let (mounted, clone) = (Scenario::new(), Scenario::new());
    let commit = mounted.commit(&[("x.json", "1")]);
    originating(&clone, "git@github.com:example-org/lib.git");
    let record = format!(
        r#"{{"$type": "board/repo/v1", "remote": "example-org/lib", "checkout": {{"repo": "{}", "follow": "main", "commit": "{commit}"}}}}"#,
        mounted.path()
    );
    let board = Scenario::new();
    board.write("repos/lib.json", Item::File(&record));
    let deep = mounted.workdir().join("docs");
    mounted.write("docs/a.md", Item::File("a"));
    assert_eq!(places(&board, &deep), [place("/repos/lib", "/docs")]);
    assert_eq!(places(&board, clone.workdir()), [place("/repos/lib", "/")]);
}

#[test]
fn only_entries_the_cache_calls_repo_records_are_read_for_their_remote() {
    let clone = Scenario::new();
    originating(&clone, "example-org/lib");
    let board = Scenario::new();
    board.write("repos/lib.json", Item::File(&repo("example-org/lib")));
    let entries = board.open().walk(&Source::WorkingTree).unwrap();
    let repositories = board.repositories();
    let (no, lib) = (|_: &str| false, |path: &str| path == "repos/lib.json");
    assert_eq!(repositories.places(&entries, &no, &no, clone.workdir()), []);
    let only = repositories.places(&entries, &no, &lib, clone.workdir());
    assert_eq!(only, [place("/repos/lib", "/")]);
}

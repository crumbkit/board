//! Places: which mount in a board reads the repository holding a directory,
//! and where the directory is beneath it (docs/architecture.md, Boards and
//! entries).

use std::path::Path;

use super::*;
use crate::Place;

/// A mount that follows `trunk`, a branch no fixture is on, so that only a
/// test that names a branch has a followed mount.
fn mount(repo: &str, commit: ObjectId) -> String {
    following(repo, "trunk", commit)
}

fn following(repo: &str, follow: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "{follow}", "commit": "{commit}"}}"#
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

/// The place at the pointer `<mount>.json`, as each fixture names its file.
fn place(mount: &str, within: &str) -> Place {
    Place {
        mount: path(mount),
        pointer: format!("{}.json", &mount[1..]),
        within: path(within),
        matched: crate::Matched::Mount,
        choice: crate::Choice::Standing,
    }
}

/// The place of a repo record found by its remote.
fn by_remote(record: &str) -> Place {
    Place {
        matched: crate::Matched::Remote,
        choice: crate::Choice::Other,
        ..place(record, "/")
    }
}

/// A linked worktree of `repository` on `branch` at `commit`, as `git
/// worktree add -b` lays it out, and its directory.
fn worktree(repository: &Scenario, branch: &str, commit: ObjectId) -> std::path::PathBuf {
    let linked = format!("{}/{branch}", repository.path());
    let git_dir = format!("{}/.git/worktrees/{branch}", repository.path());
    let inside = |file: &str| format!(".git/worktrees/{branch}/{file}");
    repository
        .write(
            &format!(".git/refs/heads/{branch}"),
            Item::File(&format!("{commit}\n")),
        )
        .write(
            &inside("HEAD"),
            Item::File(&format!("ref: refs/heads/{branch}\n")),
        )
        .write(&inside("commondir"), Item::File("../..\n"))
        .write(&inside("gitdir"), Item::File(&format!("{linked}/.git\n")))
        .write(
            &format!("{branch}/.git"),
            Item::File(&format!("gitdir: {git_dir}\n")),
        )
        .write(&format!("{branch}/a.md"), Item::File("a"));
    linked.into()
}

/// A board with a standing mount of `repository` at `/repos/lib` and, for
/// each of `changes`, a name and a phase, a record `/changes/<name>` in
/// that phase with its own mount beneath it that follows `fix-it`.
fn with_changes(repository: &Scenario, changes: &[(&str, &str)]) -> (Scenario, ObjectId) {
    let commit = repository.commit(&[("x.json", "1")]);
    let board = Scenario::new();
    board.write(
        "repos/lib.json",
        Item::File(&mount(&repository.path(), commit)),
    );
    for (name, phase) in changes {
        let record = format!(r#"{{"$type": "changes/change/v2", "phase": "{phase}"}}"#);
        let source = following(&repository.path(), "fix-it", commit);
        board
            .write(&format!("changes/{name}.json"), Item::File(&record))
            .write(&format!("changes/{name}/source.json"), Item::File(&source));
    }
    (board, commit)
}

/// The place at a change's own mount, of the kind `choice`.
fn at_change(name: &str, choice: crate::Choice) -> Place {
    Place {
        choice,
        ..place(&format!("/changes/{name}/source"), "/")
    }
}

#[test]
fn a_worktree_is_placed_at_the_mount_that_follows_its_branch_and_one_on_another_branch_at_the_standing_mount()
 {
    let repository = Scenario::new();
    let (board, commit) = with_changes(&repository, &[("fix", "build")]);
    let followed = worktree(&repository, "fix-it", commit);
    assert_eq!(
        places(&board, &followed),
        [at_change("fix", crate::Choice::Followed)]
    );
    let elsewhere = worktree(&repository, "other", commit);
    assert_eq!(places(&board, &elsewhere), [place("/repos/lib", "/")]);
}

#[test]
fn a_full_follow_ref_names_the_branch_as_its_short_name_does() {
    let repository = Scenario::new();
    let commit = repository.commit(&[("x.json", "1")]);
    let board = Scenario::new();
    let full = following(&repository.path(), "refs/heads/fix-it", commit);
    board
        .write("a.json", Item::File(&full))
        .write("b.json", Item::File(&mount(&repository.path(), commit)));
    let followed = Place {
        choice: crate::Choice::Followed,
        ..place("/a", "/")
    };
    let on_branch = worktree(&repository, "fix-it", commit);
    assert_eq!(places(&board, &on_branch), [followed]);
}

#[test]
fn a_mount_beneath_a_record_is_used_from_another_branch_when_no_standing_mount_holds_the_directory()
{
    let repository = Scenario::new();
    let (board, commit) = with_changes(&repository, &[("fix", "build")]);
    board.write("repos/lib.json", Item::Deleted);
    let elsewhere = worktree(&repository, "other", commit);
    assert_eq!(
        places(&board, &elsewhere),
        [at_change("fix", crate::Choice::Other)]
    );
}

#[test]
fn the_mount_of_a_dropped_or_done_change_is_left_out_and_its_twin_in_build_is_chosen() {
    let repository = Scenario::new();
    let ended = [("dropped", "dropped"), ("done", "done")];
    let (board, commit) = with_changes(&repository, &ended);
    let on_branch = worktree(&repository, "fix-it", commit);
    assert_eq!(places(&board, &on_branch), [place("/repos/lib", "/")]);
    let live = [("dropped", "dropped"), ("done", "done"), ("live", "build")];
    let (board, _) = with_changes(&repository, &live);
    assert_eq!(
        places(&board, &on_branch),
        [at_change("live", crate::Choice::Followed)]
    );
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
    assert_eq!(places(&board, clone.workdir()), [by_remote("/repos/lib")]);
    let docs = places(&board, &clone.workdir().join("docs"));
    assert_eq!(docs, [by_remote("/repos/lib")]);
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
        [by_remote("/repos/lib")]
    );
}

#[test]
fn a_second_clone_of_a_mounted_repository_is_placed_at_its_repo_record_by_its_origin() {
    let (mounted, clone) = (Scenario::new(), Scenario::new());
    let commit = mounted.commit(&[("x.json", "1")]);
    originating(&clone, "git@github.com:example-org/lib.git");
    let record = format!(
        r#"{{"$type": "board/repo/v1", "remote": "example-org/lib", "checkout": {{"repo": "{}", "follow": "trunk", "commit": "{commit}"}}}}"#,
        mounted.path()
    );
    let board = Scenario::new();
    board.write("repos/lib.json", Item::File(&record));
    let deep = mounted.workdir().join("docs");
    mounted.write("docs/a.md", Item::File("a"));
    assert_eq!(places(&board, &deep), [place("/repos/lib", "/docs")]);
    assert_eq!(places(&board, clone.workdir()), [by_remote("/repos/lib")]);
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
    assert_eq!(only, [by_remote("/repos/lib")]);
}

#[test]
fn a_directory_in_no_repository_has_no_origin_and_one_in_a_repository_without_origin_says_why() {
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(crate::origin(outside.path()), crate::Origin::Outside);
    let bare = Scenario::new();
    assert_eq!(
        crate::origin(bare.workdir()),
        crate::Origin::Unread("error: No such remote 'origin'".to_owned())
    );
    let originated = Scenario::new();
    originating(&originated, "git@github.com:example-org/lib.git");
    originated.write("docs/a.md", Item::File("a"));
    let url = crate::Origin::Url("git@github.com:example-org/lib.git".to_owned());
    assert_eq!(crate::origin(originated.workdir()), url);
    assert_eq!(crate::origin(&originated.workdir().join("docs")), url);
}

#[test]
fn an_origin_is_read_with_its_insteadof_applied_as_git_remote_get_url_gives_it() {
    let (plain, rewritten) = (Scenario::new(), Scenario::new());
    originating(&plain, "work:example-org/lib");
    originating(&rewritten, "work:example-org/lib");
    let rewrite = "[url \"git@github.com-work:\"]\n\tinsteadOf = work:\n";
    rewritten.write(".git/config", Item::Appended(rewrite));
    let read = |scenario: &Scenario| crate::origin(scenario.workdir());
    assert_eq!(
        read(&plain),
        crate::Origin::Url("work:example-org/lib".to_owned())
    );
    assert_eq!(
        read(&rewritten),
        crate::Origin::Url("git@github.com-work:example-org/lib".to_owned())
    );
}

#[test]
fn a_repository_whose_config_does_not_parse_is_unread_and_its_twin_that_parses_gives_its_url() {
    let (broken, whole) = (Scenario::new(), Scenario::new());
    originating(&broken, "git@github.com:example-org/lib.git");
    originating(&whole, "git@github.com:example-org/lib.git");
    broken.write(".git/config", Item::Appended("[remote \"unclosed\n"));
    let crate::Origin::Unread(why) = crate::origin(broken.workdir()) else {
        panic!("a config that does not parse is unread");
    };
    assert!(why.starts_with("error: "), "{why}");
    let url = crate::Origin::Url("git@github.com:example-org/lib.git".to_owned());
    assert_eq!(crate::origin(whole.workdir()), url);
}

/// A repository whose `origin` is `url`, holding one commit of `files`, and
/// that commit.
fn originated(url: &str, files: &[(&str, &str)]) -> (Scenario, ObjectId) {
    let repository = Scenario::new();
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    repository.write(".git/config", Item::Appended(&remote));
    let commit = repository.commit(files);
    (repository, commit)
}

#[test]
fn a_clone_is_placed_at_the_mounts_with_its_origin_and_only_at_those_whose_commit_it_holds() {
    let url = "git@github.com:example-org/lib.git";
    let (one, first) = originated(url, &[("a.json", "1")]);
    let (two, second) = originated(url, &[("a.json", "2")]);
    let board = Scenario::new();
    board
        .write("repos/one.json", Item::File(&mount(&one.path(), first)))
        .write("repos/two.json", Item::File(&mount(&two.path(), second)));
    let by_origin = |at: &str, matched| Place {
        matched,
        ..place(at, "/")
    };
    let (holding, held) = originated(url, &[("a.json", "2")]);
    assert_eq!(held, second);
    assert_eq!(
        places(&board, holding.workdir()),
        [by_origin("/repos/two", crate::Matched::OriginWithCommit)]
    );
    let (neither, _) = originated(url, &[("a.json", "3")]);
    assert_eq!(
        places(&board, neither.workdir()),
        [
            by_origin("/repos/one", crate::Matched::Origin),
            by_origin("/repos/two", crate::Matched::Origin),
        ]
    );
    let (stranger, _) = originated("git@github.com:example-org/other.git", &[("a.json", "2")]);
    assert_eq!(places(&board, stranger.workdir()), []);
}

#[test]
fn the_head_of_a_linked_worktree_is_its_own_commit_and_a_directory_in_no_repository_has_none() {
    let repository = Scenario::new();
    let side = repository.commit(&[("x.json", "1")]);
    let main = repository.commit(&[("x.json", "2")]);
    let linked = format!("{}/linked", repository.path());
    let git_dir = format!("{}/.git/worktrees/linked", repository.path());
    repository
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{side}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write("linked/docs/a.md", Item::File("a"));
    assert_eq!(crate::head_of(&Path::new(&linked).join("docs")), Some(side));
    assert_eq!(crate::head_of(repository.workdir()), Some(main));
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(crate::head_of(outside.path()), None);
}

#[test]
fn a_checkout_whose_tracked_files_hold_its_head_has_nothing_uncommitted_and_an_edited_one_has() {
    let repository = Scenario::new();
    let head = repository.commit(&[("docs/a.md", "a"), ("x.json", "1")]);
    let linked = format!("{}/linked", repository.path());
    let git_dir = format!("{}/.git/worktrees/linked", repository.path());
    repository
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{head}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write("linked/docs/a.md", Item::File("a"))
        .write("linked/x.json", Item::File("1"))
        .write("linked/untracked.md", Item::File("new"));
    let docs = Path::new(&linked).join("docs");
    assert_eq!(crate::uncommitted(&docs), Some(false));
    repository.write("linked/docs/a.md", Item::File("b"));
    assert_eq!(crate::uncommitted(&docs), Some(true));
    repository
        .write("linked/docs/a.md", Item::File("a"))
        .write("linked/x.json", Item::Deleted);
    assert_eq!(crate::uncommitted(&docs), Some(true));
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(crate::uncommitted(outside.path()), None);
}

/// A repository whose index matches its one commit, with every file on
/// disk, as `git checkout` leaves it, less the index entries `unstaged`
/// names; and the directory of its working tree.
fn checked_out(unstaged: &[&str]) -> Scenario {
    let repository = Scenario::new();
    repository.commit(&[("docs/a.md", "a"), ("x.json", "1")]);
    repository
        .write("docs/a.md", Item::File("a"))
        .write("x.json", Item::File("1"));
    let git = gix::open(repository.workdir()).unwrap();
    let tree = git.head_tree_id().unwrap().detach();
    let mut index = git.index_from_tree(&tree).unwrap();
    index.remove_entries(|_, path, _| unstaged.iter().any(|name| path == name.as_bytes()));
    index.write(gix::index::write::Options::default()).unwrap();
    repository
}

#[test]
fn a_staged_deletion_whose_file_is_still_on_disk_is_uncommitted_and_a_full_index_is_not() {
    let full = checked_out(&[]);
    assert_eq!(crate::uncommitted(full.workdir()), Some(false));
    let removed = checked_out(&["x.json"]);
    assert_eq!(crate::uncommitted(removed.workdir()), Some(true));
}

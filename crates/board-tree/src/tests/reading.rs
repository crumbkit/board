//! Reading a working-tree file: one size rule and one missing rule for every
//! reader. A file that changed after the walk is refused, carrying the bytes
//! read and the blob they make, so it is never taken for the blob the walk
//! saw.

use board_model::MAX_RECORD_BYTES;

use super::*;
use crate::{Contents, Directory, GitError, Listings, Working};

/// The blob the walk of `repository`'s working tree gave each file.
fn walked(repository: &Repository) -> impl Fn(&str) -> ObjectId {
    let entries = repository.walk(&Source::WorkingTree).unwrap();
    move |path| {
        entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.blob)
            .unwrap()
    }
}

/// The file at `path` as the reader finds it, walked or not.
fn working(repository: &Repository, path: &str) -> Working {
    repository.read_working(path).unwrap()
}

fn read(repository: &Repository, path: &str, blob: ObjectId) -> Result<Option<Contents>, GitError> {
    repository.reader().read(&Source::WorkingTree, path, blob)
}

#[test]
fn a_file_changed_after_the_walk_is_refused_with_its_new_blob_and_an_unchanged_twin_is_read() {
    let scenario = Scenario::new();
    scenario
        .write("a.json", Item::File("{}"))
        .write("b.json", Item::File("{}"));
    let repository = scenario.open();
    let blob = walked(&repository);
    scenario.write("a.json", Item::File("[]"));
    let changed = read(&repository, "a.json", blob("a.json")).unwrap_err();
    assert_eq!(changed.code(), Code::RepoUnreadable);
    assert!(changed.to_string().ends_with("; run the command again"));
    let new = Contents {
        blob: scenario.blob("[]"),
        bytes: b"[]".to_vec(),
    };
    assert_eq!(crate::latest(Err(changed)).unwrap(), Some(new));
    let unchanged = read(&repository, "b.json", blob("b.json")).unwrap();
    let old = Contents {
        blob: blob("b.json"),
        bytes: b"{}".to_vec(),
    };
    assert_eq!(unchanged, Some(old));
}

#[test]
fn a_file_removed_after_the_walk_is_unreadable_and_a_kept_twin_is_read() {
    let scenario = Scenario::new();
    scenario
        .write("a.json", Item::File("{}"))
        .write("b.json", Item::File("{}"));
    let repository = scenario.open();
    let blob = walked(&repository);
    scenario.write("a.json", Item::Deleted);
    let removed = read(&repository, "a.json", blob("a.json")).unwrap_err();
    assert_eq!(removed.code(), Code::RepoUnreadable);
    assert!(
        read(&repository, "b.json", blob("b.json"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_file_of_exactly_the_limit_is_read_and_one_byte_more_is_too_large() {
    let scenario = Scenario::new();
    let at = "a".repeat(MAX_RECORD_BYTES);
    let over = "a".repeat(MAX_RECORD_BYTES + 1);
    scenario
        .write("at.json", Item::File(&at))
        .write("over.json", Item::File(&over));
    let repository = scenario.open();
    let blob = walked(&repository);
    let Working::Read(read_at) = working(&repository, "at.json") else {
        panic!("a file of exactly the limit is read");
    };
    assert_eq!(read_at.blob, blob("at.json"));
    assert_eq!(read_at.bytes.len(), MAX_RECORD_BYTES);
    assert_eq!(working(&repository, "over.json"), Working::TooLarge);
    assert_eq!(
        read(&repository, "over.json", blob("over.json")).unwrap(),
        None,
        "a walked file too large to read has no contents"
    );
}

#[test]
fn a_missing_file_or_a_path_through_a_file_is_missing_and_a_present_twin_is_read() {
    let scenario = Scenario::new();
    scenario.write("a.json", Item::File("{}"));
    let repository = scenario.open();
    assert_eq!(working(&repository, "b.json"), Working::Missing);
    assert_eq!(working(&repository, "a.json/c.json"), Working::Missing);
    let present = Contents {
        blob: scenario.blob("{}"),
        bytes: b"{}".to_vec(),
    };
    assert_eq!(working(&repository, "a.json"), Working::Read(present));
}

#[test]
fn a_file_that_cannot_be_read_is_unreadable_and_a_readable_twin_is_read() {
    let scenario = Scenario::new();
    scenario
        .write("a.json/b.json", Item::File("{}"))
        .write("b.json", Item::File("{}"));
    let repository = scenario.open();
    let directory = repository.read_working("a.json");
    assert_eq!(directory.unwrap_err().code(), Code::RepoUnreadable);
    assert!(matches!(working(&repository, "b.json"), Working::Read(_)));
}

#[test]
fn a_file_in_a_directory_that_cannot_be_searched_is_unreadable_and_not_missing() {
    use std::os::unix::fs::PermissionsExt;
    let scenario = Scenario::new();
    scenario.write("locked/a.json", Item::File("{}"));
    let repository = scenario.open();
    assert!(matches!(
        working(&repository, "locked/a.json"),
        Working::Read(_)
    ));
    let locked = scenario.workdir().join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
    let read = repository.read_working("locked/a.json");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Root searches any directory, so the case cannot be made as root.
    if !matches!(read, Ok(Working::Read(_))) {
        assert_eq!(read.unwrap_err().code(), Code::RepoUnreadable);
    }
}

#[test]
fn a_symlink_walked_to_its_file_reads_the_file_and_one_walked_as_itself_reads_its_text() {
    let scenario = Scenario::new();
    scenario
        .write("target.json", Item::File("{}"))
        .write("link.json", Item::Symlink("target.json"));
    let repository = scenario.open();
    let blob = walked(&repository);
    assert_eq!(blob("link.json"), scenario.blob("{}"));
    let through = read(&repository, "link.json", blob("link.json")).unwrap();
    let the_file = Contents {
        blob: scenario.blob("{}"),
        bytes: b"{}".to_vec(),
    };
    assert_eq!(through, Some(the_file));
    let itself = read(&repository, "link.json", scenario.blob("target.json")).unwrap();
    let as_git_stores_it = Contents {
        blob: scenario.blob("target.json"),
        bytes: b"target.json".to_vec(),
    };
    assert_eq!(itself, Some(as_git_stores_it));
}

#[test]
fn a_pointer_changed_after_it_was_hashed_is_unreadable_and_its_unchanged_twin_is_followed() {
    let scenario = Scenario::new();
    let to = |target: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#);
    scenario
        .write("a/y.json", Item::File("{}"))
        .write("b/y.json", Item::File("{}"))
        .write("to_a.json", Item::File(&to("/a")))
        .write("to_b.json", Item::File(&to("/b")));
    let repository = scenario.repositories();
    let mut known = |_: &str, _: &board_model::Stat| None;
    let mut resolver = repository
        .resolver(&mut known, Listings::default())
        .unwrap();
    // Hashes both pointers, without following them, before one changes.
    resolver.resolve_pointer(&path("/to_a")).unwrap();
    resolver.resolve_pointer(&path("/to_b")).unwrap();
    scenario.write("to_a.json", Item::File(&to("/b")));
    let changed = resolver.resolve(&path("/to_a/y")).unwrap_err();
    assert_eq!(changed.code(), Code::RepoUnreadable);
    let unchanged = resolver.resolve(&path("/to_b/y")).unwrap();
    assert_eq!(unchanged.path, "b/y.json");
}

#[test]
fn a_path_in_or_at_a_submodule_is_a_submodule_by_the_walks_rule_and_any_other_directory_holds_entries()
 {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[(b"linked", Item::Submodule)]);
    scenario
        .track(commit)
        .write("linked/x", Item::File(""))
        .write("linked/x", Item::Deleted)
        .write("nested", Item::Submodule)
        .write("plain/x.json", Item::File("{}"))
        .write("file.json", Item::File("{}"))
        .write("link", Item::Symlink("plain"))
        .write("fifo", Item::Fifo);
    let repository = scenario.open();
    let directory = |path: &str| repository.working_directory(path).unwrap();
    assert_eq!(directory("linked"), Some(Directory::Submodule));
    assert_eq!(directory("nested"), Some(Directory::Submodule));
    assert_eq!(directory("nested/x.json"), Some(Directory::Submodule));
    assert_eq!(directory("plain"), Some(Directory::Entries));
    // The walk lists a symlink and nothing through it.
    assert_eq!(directory("link/x.json"), Some(Directory::Entries));
    assert_eq!(directory("link"), None);
    assert_eq!(directory("file.json"), None);
    assert_eq!(directory("file.json/x"), None);
    // The walk lists nothing that is not a file, directory or symlink.
    assert_eq!(directory("fifo"), Some(Directory::Entries));
    assert_eq!(directory("absent"), None);
}

#[test]
fn a_name_in_another_case_is_entries_where_the_file_system_folds_case_and_its_spelling_is_seen() {
    let scenario = Scenario::new();
    scenario.write("Plain/X.json", Item::File("{}"));
    let repository = scenario.open();
    let directory = |path: &str| repository.working_directory(path).unwrap();
    assert_eq!(directory("Plain/X.json"), None);
    assert_eq!(directory("Plain"), Some(Directory::Entries));
    // A case-sensitive file system finds nothing at these paths, so both
    // kinds of file system are checked: each must answer as the walk lists.
    let folds = scenario.workdir().join("plain/x.json").exists();
    for path in ["plain/X.json", "Plain/x.json", "plain"] {
        let found = folds.then_some(Directory::Entries);
        assert_eq!(directory(path), found, "{path}");
    }
}

#[test]
fn a_board_is_discovered_only_where_its_configuration_is_listed_in_its_own_spelling() {
    let found = |config: &str| {
        let scenario = Scenario::new();
        scenario
            .write(config, Item::File("{}"))
            .write("deep/x.md", Item::File(""));
        let inside = scenario.workdir().join("deep");
        let root = scenario.workdir().canonicalize().unwrap();
        crate::discover(&inside).map(|found| found == root)
    };
    assert_eq!(found(".board/board.json"), Some(true));
    // A case-insensitive file system opens these paths as .board/board.json;
    // the walk lists them as written, so neither holds a board.
    for misspelt in [".Board/board.json", ".board/Board.json"] {
        assert_eq!(found(misspelt), None, "{misspelt}");
    }
    // A directory named board.json is listed but holds no configuration.
    assert_eq!(found(".board/board.json/a.md"), None);
}

#[test]
fn a_directory_on_the_way_that_cannot_be_listed_is_refused_and_one_that_can_holds_entries() {
    let scenario = Scenario::new();
    scenario
        .write("locked", Item::Locked)
        .write("searchable/x.json", Item::File("{}"))
        .write("searchable", Item::Searchable)
        .write("open/x.json", Item::File("{}"));
    let repository = scenario.open();
    let directory = |path: &str| repository.working_directory(path);
    assert_eq!(directory("open").unwrap(), Some(Directory::Entries));
    assert_eq!(directory("open/x.json").unwrap(), None);
    // A superuser lists any directory, so the case cannot be made as root.
    if std::fs::read_dir(scenario.workdir().join("locked")).is_ok() {
        return;
    }
    assert!(matches!(directory("locked"), Err(GitError::File { .. })));
    // The walk cannot list the directory, so it never sees the file.
    assert!(matches!(
        directory("searchable/x.json"),
        Err(GitError::File { .. })
    ));
}

#[test]
fn a_path_the_walk_leaves_out_as_ignored_is_ignored_and_a_tracked_twin_is_seen() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("keep.log", ""), ("build/keep.txt", "")]);
    scenario
        .track(commit)
        .write(".gitignore", Item::File("*.log\nbuild/\nskipped/\n"))
        .write("keep.log", Item::File(""))
        .write("new.log", Item::File(""))
        .write("build/keep.txt", Item::File(""))
        .write("build/new.txt", Item::File(""))
        .write("skipped/x.json", Item::File("{}"))
        .write("seen/x.json", Item::File("{}"));
    let repository = scenario.open();
    let walked: Vec<String> = repository
        .walk(&Source::WorkingTree)
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    let directory = |path: &str| repository.working_directory(path).unwrap();
    for path in ["new.log", "build/new.txt", "skipped/x.json", "skipped"] {
        assert_eq!(directory(path), Some(Directory::Ignored), "{path}");
        assert!(!walked.iter().any(|walked| walked == path), "{path}");
    }
    for path in ["keep.log", "build/keep.txt", "seen/x.json"] {
        assert_eq!(directory(path), None, "{path}");
        assert!(walked.iter().any(|walked| walked == path), "{path}");
    }
    assert_eq!(directory("build"), Some(Directory::Entries));
}

#[test]
fn a_file_beneath_a_symlink_to_a_directory_outside_the_repository_is_missing_and_one_beneath_a_directory_is_read()
 {
    let outer = Scenario::new();
    outer
        .write("outside/a.json", Item::File("{}"))
        .write("outside/b.json", Item::Symlink("a.json"));
    let scenario = Scenario::within(&outer);
    scenario
        .write("real/linked", Item::Symlink("../../outside"))
        .write("real/a.json", Item::File("{}"));
    let repository = scenario.open();
    for path in ["real/linked/a.json", "real/linked/b.json"] {
        assert_eq!(working(&repository, path), Working::Missing, "{path}");
        let through = repository.reader().read_through(path).unwrap();
        assert_eq!(through, (Working::Missing, false), "{path}");
    }
    let read = Working::Read(Contents {
        blob: scenario.blob("{}"),
        bytes: b"{}".to_vec(),
    });
    assert_eq!(working(&repository, "real/a.json"), read);
    assert_eq!(
        repository.reader().read_through("real/a.json").unwrap(),
        (read, false)
    );
}

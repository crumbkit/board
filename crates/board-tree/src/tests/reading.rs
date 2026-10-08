//! Reading a working-tree file the walk listed: a file that changed after
//! the walk is refused, carrying the bytes read and the blob they make, so it
//! is never taken for the blob the walk saw.

use board_model::MAX_RECORD_BYTES;

use super::*;
use crate::{Contents, GitError};

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

fn read(repository: &Repository, path: &str, blob: ObjectId) -> Result<Option<Contents>, GitError> {
    repository.read(&Source::WorkingTree, path, blob)
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
fn a_file_of_exactly_the_limit_is_read_with_its_blob_and_one_byte_more_is_too_large() {
    let scenario = Scenario::new();
    let at = "a".repeat(MAX_RECORD_BYTES);
    let over = "a".repeat(MAX_RECORD_BYTES + 1);
    scenario
        .write("at.json", Item::File(&at))
        .write("over.json", Item::File(&over));
    let repository = scenario.open();
    let blob = walked(&repository);
    let read_at = read(&repository, "at.json", blob("at.json"))
        .unwrap()
        .unwrap();
    assert_eq!(read_at.blob, blob("at.json"));
    assert_eq!(read_at.bytes.len(), MAX_RECORD_BYTES);
    assert_eq!(
        read(&repository, "over.json", blob("over.json")).unwrap(),
        None
    );
}

#[test]
fn a_symlink_is_read_as_its_target_path_with_the_blob_the_walk_gave_it() {
    let scenario = Scenario::new();
    scenario
        .write("target.json", Item::File("{}"))
        .write("link.json", Item::Symlink("target.json"));
    let repository = scenario.open();
    let blob = walked(&repository);
    let link = read(&repository, "link.json", blob("link.json")).unwrap();
    let as_git_stores_it = Contents {
        blob: scenario.blob("target.json"),
        bytes: b"target.json".to_vec(),
    };
    assert_eq!(link, Some(as_git_stores_it));
}

#[test]
fn a_pointer_changed_after_its_listing_is_unreadable_and_its_unchanged_twin_is_followed() {
    let scenario = Scenario::new();
    let to = |target: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#);
    scenario
        .write("a/y.json", Item::File("{}"))
        .write("b/y.json", Item::File("{}"))
        .write("to_a.json", Item::File(&to("/a")))
        .write("to_b.json", Item::File(&to("/b")));
    let repository = scenario.open();
    let mut known = |_: &str, _: &board_model::Stat| None;
    let mut resolver = repository.resolver(&mut known).unwrap();
    // Lists the root, hashing both pointers, before one of them changes.
    resolver.resolve(&path("/a/y")).unwrap();
    scenario.write("to_a.json", Item::File(&to("/b")));
    let changed = resolver.resolve(&path("/to_a/y")).unwrap_err();
    assert_eq!(changed.code(), Code::RepoUnreadable);
    let unchanged = resolver.resolve(&path("/to_b/y")).unwrap();
    assert_eq!(unchanged.path, "b/y.json");
}

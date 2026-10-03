//! Resolution and walking over real repositories built by the scenario
//! builder (docs/architecture.md, Testing).

use std::collections::HashSet;

use board_model::{Code, Location, LogicalPath, Object, ObjectId, Source};
use proptest::prelude::*;

use crate::scenario::{Item, Scenario};
use crate::{Entry, Repository, Status};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn resolve(scenario: &Scenario, commit: ObjectId, text: &str) -> Result<Location, Code> {
    scenario
        .open()
        .resolve(commit, &path(text))
        .map_err(|error| error.code())
}

/// Asserts that `admitted` resolves to the physical `physical` and that
/// `refused` is refused with `code`.
fn pair(scenario: &Scenario, commit: ObjectId, admitted: (&str, &str), refused: &str, code: Code) {
    let location = resolve(scenario, commit, admitted.0).unwrap();
    assert_eq!(location.path, admitted.1);
    assert_eq!(resolve(scenario, commit, refused).unwrap_err(), code);
}

/// The walk's entries as (physical path, logical path, status).
fn walk(scenario: &Scenario, commit: ObjectId) -> Vec<(String, Option<String>, Status)> {
    scenario
        .open()
        .walk(commit)
        .unwrap()
        .into_iter()
        .map(|entry: Entry| {
            let logical = entry.logical.map(String::from);
            (entry.path, logical, entry.status)
        })
        .collect()
}

fn ok(physical: &str, logical: &str) -> (String, Option<String>, Status) {
    (physical.to_owned(), Some(logical.to_owned()), Status::Ok)
}

fn degraded(physical: &str, logical: Option<&str>, code: Code) -> (String, Option<String>, Status) {
    (
        physical.to_owned(),
        logical.map(str::to_owned),
        Status::Degraded(code),
    )
}

#[test]
fn a_plain_path_resolves_to_its_record_in_the_commit() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("work/x.json", "{}")]);
    let location = resolve(&scenario, commit, "/work/x").unwrap();
    assert_eq!(location.path, "work/x.json");
    assert_eq!(location.object, Object::Blob(scenario.blob("{}")));
    assert_eq!(location.board.source, Source::Commit(commit));
    assert_eq!(location.board.repository, scenario.open().path());
    pair(
        &scenario,
        commit,
        ("/work/x", "work/x.json"),
        "/work/y",
        Code::PathNotFound,
    );
}

#[test]
fn a_record_is_addressed_only_without_json() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("x.json", "{}")]);
    pair(
        &scenario,
        commit,
        ("/x", "x.json"),
        "/x.json",
        Code::PathNotFound,
    );
}

#[test]
fn any_other_file_is_addressed_by_its_whole_name() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("notes.md", "# notes")]);
    pair(
        &scenario,
        commit,
        ("/notes.md", "notes.md"),
        "/notes",
        Code::PathNotFound,
    );
}

#[test]
fn a_directory_and_the_root_resolve_as_directories() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("work/x.json", "{}")]);
    let work = resolve(&scenario, commit, "/work").unwrap();
    assert_eq!(
        (work.path.as_str(), work.object),
        ("work", Object::Directory)
    );
    let root = resolve(&scenario, commit, "/").unwrap();
    assert_eq!((root.path.as_str(), root.object), ("", Object::Directory));
}

#[test]
fn a_path_cannot_continue_through_a_file() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("work/x.json", "{}"), ("notes.md", "")]);
    pair(
        &scenario,
        commit,
        ("/work/x", "work/x.json"),
        "/notes.md/x",
        Code::PathNotFound,
    );
}

#[test]
fn a_record_and_a_directory_at_one_path_are_shadowed() {
    let scenario = Scenario::new();
    let clear = scenario.commit(&[("a.json", "{}"), ("c/b.json", "{}")]);
    assert_eq!(resolve(&scenario, clear, "/a").unwrap().path, "a.json");
    let shadowed = scenario.commit(&[("a.json", "{}"), ("a/b.json", "{}")]);
    assert_eq!(
        resolve(&scenario, shadowed, "/a").unwrap_err(),
        Code::PathShadowed
    );
    assert_eq!(
        resolve(&scenario, shadowed, "/a/b").unwrap_err(),
        Code::PathShadowed
    );
}

#[test]
fn walking_marks_both_shadowing_entries_and_everything_beneath() {
    let scenario = Scenario::new();
    let clear = scenario.commit(&[("a.json", "{}"), ("c/b.json", "{}")]);
    assert_eq!(
        walk(&scenario, clear),
        [ok("a.json", "/a"), ok("c/b.json", "/c/b")]
    );
    let shadowed = scenario.commit(&[("a.json", "{}"), ("a/b.json", "{}")]);
    assert_eq!(
        walk(&scenario, shadowed),
        [
            degraded("a.json", Some("/a"), Code::PathShadowed),
            degraded("a/b.json", Some("/a/b"), Code::PathShadowed),
        ]
    );
}

#[test]
fn names_differing_only_by_case_are_degraded_but_still_resolve_exactly() {
    let scenario = Scenario::new();
    let clear = scenario.commit(&[("A.json", "1"), ("b.json", "2")]);
    assert_eq!(
        walk(&scenario, clear),
        [ok("A.json", "/A"), ok("b.json", "/b")]
    );
    let colliding = scenario.commit(&[("A.json", "1"), ("a.json", "2")]);
    assert_eq!(
        walk(&scenario, colliding),
        [
            degraded("A.json", Some("/A"), Code::PathCaseCollision),
            degraded("a.json", Some("/a"), Code::PathCaseCollision),
        ]
    );
    let location = resolve(&scenario, colliding, "/a").unwrap();
    assert_eq!(location.object, Object::Blob(scenario.blob("2")));
}

#[test]
fn directories_differing_only_by_case_degrade_their_contents() {
    let scenario = Scenario::new();
    let colliding = scenario.commit(&[("Work/x.json", "{}"), ("work/y.json", "{}")]);
    assert_eq!(
        walk(&scenario, colliding),
        [
            degraded("Work/x.json", Some("/Work/x"), Code::PathCaseCollision),
            degraded("work/y.json", Some("/work/y"), Code::PathCaseCollision),
        ]
    );
}

#[test]
fn a_name_that_is_not_a_segment_is_malformed_with_everything_beneath() {
    let scenario = Scenario::new();
    let clear = scenario.commit(&[("a b.json", "{}"), ("d b/x.json", "{}")]);
    assert_eq!(
        walk(&scenario, clear),
        [ok("a b.json", "/a b"), ok("d b/x.json", "/d b/x")]
    );
    let malformed = scenario.commit(&[("a\tb.json", "{}"), ("d\tb/x.json", "{}"), (".json", "{}")]);
    assert_eq!(
        walk(&scenario, malformed),
        [
            degraded(".json", None, Code::PathMalformed),
            degraded("a\tb.json", None, Code::PathMalformed),
            degraded("d\tb/x.json", None, Code::PathMalformed),
        ]
    );
}

#[test]
fn a_name_that_is_not_utf8_is_malformed_and_shown_lossily() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[
        (b"a\xff.json", Item::File("{}")),
        (b"b.json", Item::File("{}")),
    ]);
    assert_eq!(
        walk(&scenario, commit),
        [
            degraded("a\u{fffd}.json", None, Code::PathMalformed),
            ok("b.json", "/b")
        ]
    );
}

#[test]
fn a_path_over_4096_bytes_is_malformed() {
    // Fifteen 255-byte directories, a 253-byte one and `x` make 4,096 bytes;
    // sixteen 255-byte directories and `x` make 4,098.
    let directories = vec!["d".repeat(255); 15].join("/");
    let fits = format!("{directories}/{}/x.json", "e".repeat(253));
    let over = format!("{directories}/{}/x.json", "d".repeat(255));
    let scenario = Scenario::new();
    let commit = scenario.commit(&[(fits.as_str(), "{}"), (over.as_str(), "{}")]);
    let statuses: Vec<Status> = walk(&scenario, commit)
        .into_iter()
        .map(|entry| entry.2)
        .collect();
    // The longer path sorts first: its sixteenth directory starts with `d`.
    assert_eq!(
        statuses,
        [Status::Degraded(Code::PathMalformed), Status::Ok]
    );
}

#[test]
fn symlinks_and_submodules_are_entries_that_are_never_followed() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[
        (b"work/x.json", Item::File("{}")),
        (b"s.json", Item::Symlink("work")),
        (b"m", Item::Submodule),
    ]);
    assert_eq!(
        walk(&scenario, commit),
        [
            ok("m", "/m"),
            ok("s.json", "/s.json"),
            ok("work/x.json", "/work/x")
        ]
    );
    let link = resolve(&scenario, commit, "/s.json").unwrap();
    assert_eq!(link.object, Object::Blob(scenario.blob("work")));
    pair(
        &scenario,
        commit,
        ("/work/x", "work/x.json"),
        "/s.json/x",
        Code::PathNotFound,
    );
    pair(&scenario, commit, ("/m", "m"), "/m/x", Code::PathNotFound);
}

#[test]
fn resolution_reads_the_commit_it_is_given() {
    let scenario = Scenario::new();
    let first = scenario.commit(&[("x.json", "1")]);
    let second = scenario.commit(&[("x.json", "2")]);
    let blob = |commit| resolve(&scenario, commit, "/x").unwrap().object;
    assert_eq!(blob(first), Object::Blob(scenario.blob("1")));
    assert_eq!(blob(second), Object::Blob(scenario.blob("2")));
}

#[test]
fn a_missing_commit_or_repository_is_unreadable() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("x.json", "{}")]);
    assert!(resolve(&scenario, commit, "/x").is_ok());
    let missing = ObjectId::Sha1([7; 20]);
    assert_eq!(
        resolve(&scenario, missing, "/x").unwrap_err(),
        Code::TreeUnreadable
    );
    let error = scenario.open().walk(missing).unwrap_err();
    assert_eq!(error.code(), Code::TreeUnreadable);

    let empty = tempfile::tempdir().unwrap();
    assert!(Repository::open(scenario.open().path()).is_ok());
    assert_eq!(
        Repository::open(empty.path()).unwrap_err().code(),
        Code::TreeUnreadable
    );
}

/// A small tree: up to eight files, each one to three segments drawn from a
/// few names, so that records, directories and files often meet at one path.
fn files() -> impl Strategy<Value = Vec<String>> {
    let segment = prop::sample::select(vec!["a", "b", "A", "a.json", "b.md", "a b"]);
    let file = prop::collection::vec(segment, 1..4).prop_map(|segments| segments.join("/"));
    prop::collection::btree_set(file, 1..8).prop_map(|files| files.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn every_ok_entry_resolves_to_itself_and_no_path_repeats(files in files()) {
        let scenario = Scenario::new();
        let items: Vec<(&[u8], Item<'_>)> = files
            .iter()
            .map(|file| (file.as_bytes(), Item::File(file.as_str())))
            .collect();
        // A file path that is also a directory is not a tree git can hold.
        prop_assume!(files.iter().all(|file| !files.iter().any(|other| other.starts_with(&format!("{file}/")))));
        let commit = scenario.commit_items(&items);
        let repository = scenario.open();
        let entries = repository.walk(commit).unwrap();
        let physical: HashSet<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        prop_assert_eq!(physical.len(), entries.len());
        for entry in &entries {
            if entry.status == Status::Ok {
                let logical = entry.logical.as_ref().unwrap();
                let location = repository.resolve(commit, logical).unwrap();
                prop_assert_eq!(&location.path, &entry.path);
                prop_assert_eq!(location.object, Object::Blob(entry.blob));
            }
        }
    }
}

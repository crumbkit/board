//! The working-tree source: every file git sees, with uncommitted edits
//! marked (docs/architecture.md, Boards and entries).

use super::*;
use crate::{Change, Entry};

/// The working tree's entries as (physical path, change).
fn changes(scenario: &Scenario) -> Vec<(String, Change)> {
    scenario
        .open()
        .walk(&Source::WorkingTree)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.path, entry.change))
        .collect()
}

fn change(path: &str, change: Change) -> (String, Change) {
    (path.to_owned(), change)
}

#[test]
fn a_file_in_the_working_tree_resolves_to_the_blob_git_would_store() {
    let scenario = Scenario::new();
    scenario.write("work/x.json", Item::File("{\"a\": 1}\n"));
    let location = resolve(&scenario, WorkingTree, "/work/x").unwrap();
    assert_eq!(location.path, "work/x.json");
    assert_eq!(location.object, Object::Blob(scenario.blob("{\"a\": 1}\n")));
    assert_eq!(location.board.source, Source::WorkingTree);
    pair(
        &scenario,
        WorkingTree,
        ("/work/x", "work/x.json"),
        "/work/y",
        Code::PathNotFound,
    );
}

#[test]
fn the_git_directory_is_never_an_entry() {
    let scenario = Scenario::new();
    scenario.commit(&[("x.json", "{}")]);
    scenario.write("x.json", Item::File("{}"));
    assert_eq!(walk(&scenario, WorkingTree), [ok("x.json", "/x")]);
    assert_eq!(
        resolve(&scenario, WorkingTree, "/.git").unwrap_err(),
        Code::PathNotFound
    );
}

#[test]
fn an_ignored_untracked_file_is_not_an_entry() {
    let scenario = Scenario::new();
    scenario
        .write(".gitignore", Item::File("*.log\n"))
        .write("a.txt", Item::File(""))
        .write("a.log", Item::File(""));
    assert_eq!(
        walk(&scenario, WorkingTree),
        [ok(".gitignore", "/.gitignore"), ok("a.txt", "/a.txt")]
    );
    pair(
        &scenario,
        WorkingTree,
        ("/a.txt", "a.txt"),
        "/a.log",
        Code::PathNotFound,
    );
}

#[test]
fn a_tracked_file_is_an_entry_even_when_ignored() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("keep.log", ""), ("build/keep.txt", "")]);
    scenario
        .track(commit)
        .write(".gitignore", Item::File("*.log\nbuild/\n"))
        .write("keep.log", Item::File(""))
        .write("new.log", Item::File(""))
        .write("build/keep.txt", Item::File(""))
        .write("build/new.txt", Item::File(""));
    assert_eq!(
        walk(&scenario, WorkingTree),
        [
            ok(".gitignore", "/.gitignore"),
            ok("build/keep.txt", "/build/keep.txt"),
            ok("keep.log", "/keep.log"),
        ]
    );
}

#[test]
fn an_entry_that_differs_from_head_is_uncommitted() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("same.json", "1"), ("edited.json", "1")]);
    scenario
        .track(commit)
        .write("same.json", Item::File("1"))
        .write("edited.json", Item::File("2"))
        .write("new.json", Item::File("1"));
    assert_eq!(
        changes(&scenario),
        [
            change("edited.json", Change::Uncommitted),
            change("new.json", Change::Uncommitted),
            change("same.json", Change::Committed),
        ]
    );
}

#[test]
fn before_the_first_commit_every_entry_is_uncommitted() {
    let scenario = Scenario::new();
    scenario.write("x.json", Item::File("{}"));
    assert_eq!(changes(&scenario), [change("x.json", Change::Uncommitted)]);
}

#[test]
fn a_symlink_in_the_working_tree_is_a_file_holding_its_target() {
    let scenario = Scenario::new();
    scenario
        .write("work/x.json", Item::File("{}"))
        .write("s", Item::Symlink("work"));
    let link = resolve(&scenario, WorkingTree, "/s").unwrap();
    assert_eq!(link.object, Object::Blob(scenario.blob("work")));
    pair(
        &scenario,
        WorkingTree,
        ("/work/x", "work/x.json"),
        "/s/x",
        Code::PathNotFound,
    );
}

#[test]
fn a_nested_repository_is_a_submodule() {
    let scenario = Scenario::new();
    scenario
        .write("plain/x.json", Item::File("{}"))
        .write("nested", Item::Submodule);
    assert_eq!(
        walk(&scenario, WorkingTree),
        [
            degraded("nested", Some("/nested"), Code::RepoSubmodule),
            ok("plain/x.json", "/plain/x"),
        ]
    );
    assert_eq!(
        resolve(&scenario, WorkingTree, "/nested/x").unwrap_err(),
        Code::RepoSubmodule
    );
}

#[test]
fn a_repository_without_a_working_tree_cannot_be_read_as_one() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("x.json", "{}")]);
    assert!(scenario.open().walk(&Source::WorkingTree).is_ok());
    let bare = tempfile::tempdir().unwrap();
    gix::init_bare(bare.path()).unwrap();
    let repository = Repository::open(bare.path()).unwrap();
    let error = repository.walk(&Source::WorkingTree).unwrap_err();
    assert_eq!(error.code(), Code::RepoUnreadable);
    assert!(scenario.open().walk(&commit.source()).is_ok());
}

/// Files whose names cannot collide on a case-insensitive file system.
fn checkout() -> impl Strategy<Value = Vec<String>> {
    let segment = prop::sample::select(vec!["a", "b", "a.json", "b.md", "a b"]);
    let file = prop::collection::vec(segment, 1..4).prop_map(|segments| segments.join("/"));
    prop::collection::btree_set(file, 1..8).prop_map(|files| files.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn a_clean_checkout_walks_exactly_as_its_commit(files in checkout()) {
        prop_assume!(files.iter().all(|file| !files.iter().any(|other| other.starts_with(&format!("{file}/")))));
        let scenario = Scenario::new();
        let pairs: Vec<(&str, &str)> = files.iter().map(|file| (file.as_str(), file.as_str())).collect();
        let commit = scenario.commit(&pairs);
        scenario.track(commit);
        for file in &files {
            scenario.write(file, Item::File(file));
        }
        let repository = scenario.open();
        let committed = repository.walk(&commit.source()).unwrap();
        let working = repository.walk(&Source::WorkingTree).unwrap();
        prop_assert!(working.iter().all(|entry| entry.change == Change::Committed));
        prop_assert!(working.iter().all(|entry| entry.stat.is_some()), "a working-tree file has metadata");
        let unstated: Vec<Entry> = working.into_iter().map(|entry| Entry { stat: None, ..entry }).collect();
        prop_assert_eq!(unstated, committed);
    }
}

#[test]
fn a_file_that_cannot_be_read_is_one_degraded_entry_and_the_walk_goes_on() {
    let readable = Scenario::new();
    readable
        .write("a.json", Item::File("{}"))
        .write("b.json", Item::File("{}"));
    assert_eq!(
        walk(&readable, WorkingTree),
        [ok("a.json", "/a"), ok("b.json", "/b")]
    );

    let locked = Scenario::new();
    locked
        .write("a.json", Item::Unreadable("{}"))
        .write("b.json", Item::File("{}"));
    // A superuser reads a mode-000 file anyway, so the fixture cannot stage
    // the failure; CI runs as an ordinary user.
    let path = locked.open().path().join("a.json");
    if std::fs::File::open(path).is_ok() {
        return;
    }
    assert_eq!(
        walk(&locked, WorkingTree),
        [
            degraded("a.json", Some("/a"), Code::RepoUnreadable),
            ok("b.json", "/b"),
        ]
    );
    let entries = locked.open().walk(&Source::WorkingTree).unwrap();
    assert_eq!(entries[0].blob, None);
    assert_eq!(
        resolve(&locked, WorkingTree, "/a").unwrap_err(),
        Code::RepoUnreadable
    );
}

#[test]
fn a_large_file_hashes_to_the_blob_git_would_store() {
    let text = "0123456789abcdef".repeat(70_000) + "tail";
    let scenario = Scenario::new();
    scenario.write("big.bin", Item::File(&text));
    let location = resolve(&scenario, WorkingTree, "/big.bin").unwrap();
    assert_eq!(location.object, Object::Blob(scenario.blob(&text)));
}

#[test]
fn a_submodule_in_the_working_tree_carries_no_blob() {
    let scenario = Scenario::new();
    let commit = scenario.commit_items(&[(b"m", Item::Submodule)]);
    scenario.track(commit).write("m", Item::Submodule);
    let entries = scenario.open().walk(&Source::WorkingTree).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].blob, None);
    assert_eq!(entries[0].status, Status::Degraded(Code::RepoSubmodule));
}

#[test]
fn a_file_whose_blob_is_known_is_not_hashed_and_the_rest_are() {
    let scenario = Scenario::new();
    scenario
        .write("known.json", Item::File("{}"))
        .write("dir/unknown.json", Item::File("[]"));
    let stand_in = scenario.blob("not what the file holds");
    let mut asked = Vec::new();
    let entries = scenario
        .open()
        .walk_working_tree(&mut |path, stat| {
            asked.push((path.to_owned(), stat.size));
            (path == "known.json").then_some(stand_in)
        })
        .unwrap();
    asked.sort();
    assert_eq!(
        asked,
        [
            ("dir/unknown.json".to_owned(), 2),
            ("known.json".to_owned(), 2)
        ]
    );
    let blobs: Vec<_> = entries
        .iter()
        .map(|entry| (&*entry.path, entry.blob))
        .collect();
    assert_eq!(
        blobs,
        [
            ("dir/unknown.json", Some(scenario.blob("[]"))),
            ("known.json", Some(stand_in)),
        ],
        "the known blob is taken as given, which shows the file was not hashed"
    );
}

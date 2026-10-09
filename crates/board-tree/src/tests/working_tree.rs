//! The working-tree source: every file git sees (docs/architecture.md,
//! Boards and entries).

use super::*;
use crate::{Contents, Entry};

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
fn a_path_beside_a_record_that_cannot_be_read_is_unreadable_as_the_walk_reports_it() {
    let readable = Scenario::new();
    readable
        .write("x.json", Item::File("{}"))
        .write("x/c.json", Item::File("{}"));
    assert_eq!(
        resolve(&readable, WorkingTree, "/x/c").unwrap().path,
        "x/c.json"
    );

    let locked = Scenario::new();
    locked
        .write("x.json", Item::Unreadable("{}"))
        .write("x/c.json", Item::File("{}"));
    // A superuser reads a mode-000 file anyway, so the fixture cannot stage
    // the failure; CI runs as an ordinary user.
    let path = locked.open().path().join("x.json");
    if std::fs::File::open(path).is_ok() {
        return;
    }
    assert_eq!(
        resolve(&locked, WorkingTree, "/x/c").unwrap_err(),
        Code::RepoUnreadable
    );
    assert_eq!(
        walk(&locked, WorkingTree),
        [
            degraded("x.json", Some("/x"), Code::RepoUnreadable),
            degraded("x/c.json", Some("/x/c"), Code::RepoUnreadable),
        ]
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
        .write("dir/unknown.json", Item::File("[]"))
        .write("notes.md", Item::File("[]"));
    let stand_in = scenario.blob("not what the file holds");
    let mut asked = Vec::new();
    let mut kept = Vec::new();
    let entries = scenario
        .open()
        .walk_working_tree(
            &mut |path, stat| {
                asked.push((path.to_owned(), stat.size));
                (path == "known.json").then_some(stand_in)
            },
            &mut |path, contents| kept.push((path.to_owned(), contents)),
        )
        .unwrap();
    asked.sort();
    assert_eq!(
        asked,
        [
            ("dir/unknown.json".to_owned(), 2),
            ("known.json".to_owned(), 2),
            ("notes.md".to_owned(), 2),
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
            ("notes.md", Some(scenario.blob("[]"))),
        ],
        "the known blob is taken as given, which shows the file was not hashed"
    );
    let contents = Contents {
        blob: scenario.blob("[]"),
        bytes: b"[]".to_vec(),
    };
    assert_eq!(
        kept,
        [("dir/unknown.json".to_owned(), contents)],
        "only the JSON file hashed is kept, with the blob of its bytes"
    );
}

#[test]
fn resolution_takes_a_known_blob_as_given_and_hashes_the_rest() {
    let scenario = Scenario::new();
    scenario
        .write("work/known.md", Item::File("{}"))
        .write("work/other.json", Item::File("[]"));
    let stand_in = scenario.blob("not what the file holds");
    let repository = scenario.repositories();
    let mut known =
        |path: &str, _: &board_model::Stat| (path == "work/known.md").then_some(stand_in);
    let mut resolver = repository.resolver(&mut known).unwrap();
    let mut resolve = |text: &str| resolver.resolve(&path(text)).unwrap().object;
    assert_eq!(resolve("/work/known.md"), Object::Blob(stand_in));
    assert_eq!(resolve("/work/other"), Object::Blob(scenario.blob("[]")));
}

#[test]
fn the_repository_root_is_found_from_below_and_not_outside_a_repository() {
    let scenario = Scenario::new();
    scenario.write("docs/a.md", Item::File(""));
    let root = scenario.workdir().canonicalize().unwrap();
    let below = scenario.workdir().join("docs");
    assert_eq!(crate::repository_root(&below), Some(root));
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(crate::repository_root(outside.path()), None);
}

/// The segments past the deepest ancestor of `text` that resolves, and the
/// physical path a file there would have.
fn ancestor(scenario: &Scenario, text: &str) -> (Vec<String>, Option<String>) {
    let ancestor = scenario.repositories().ancestor(&path(text)).unwrap();
    (ancestor.rest.clone(), ancestor.path())
}

#[test]
fn a_missing_path_lies_beneath_its_deepest_ancestor_and_nothing_lies_beneath_a_file() {
    let scenario = Scenario::new();
    scenario
        .write("docs/a.md", Item::File("text"))
        .write("tasks/a.json", Item::File("{}"));
    let owned = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ancestor(&scenario, "/new.md"),
        (owned(&["new.md"]), Some("new.md".to_owned())),
        "beneath the root"
    );
    assert_eq!(
        ancestor(&scenario, "/docs/sub/new.md"),
        (
            owned(&["sub", "new.md"]),
            Some("docs/sub/new.md".to_owned())
        ),
        "beneath a directory"
    );
    assert_eq!(
        ancestor(&scenario, "/tasks/a/notes/x"),
        (owned(&["notes", "x"]), Some("tasks/a/notes/x".to_owned())),
        "beside a record"
    );
    assert_eq!(
        ancestor(&scenario, "/docs/a.md/x"),
        (owned(&["x"]), None),
        "beneath a file that is not JSON"
    );
}

#[test]
fn a_write_beside_a_directory_is_refused_as_shadowed_only_when_it_writes_a_pointer() {
    let scenario = Scenario::new();
    scenario.write("x/c.json", Item::File("{}"));
    let repository = scenario.repositories();
    let target = repository.target(&path("/x"), false).unwrap();
    assert_eq!(target.path, "x.json");
    match repository.target(&path("/x"), true) {
        Err(crate::WriteError::Refused { code, .. }) => assert_eq!(code, Code::PathShadowed),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Where a write to `text` lands, or the code that refuses it.
fn target(scenario: &Scenario, text: &str, is_pointer: bool) -> Result<String, Code> {
    let repository = scenario.repositories();
    let mut snapshot = repository.snapshot().unwrap();
    snapshot
        .target(&path(text), is_pointer)
        .map(|target| target.path)
        .map_err(|error| error.code())
}

#[test]
fn a_write_into_a_directory_beside_an_invalid_pointer_is_refused_as_shadowed() {
    let scenario = Scenario::new();
    scenario
        .write("x.json", Item::File("{}"))
        .write("x/c.json", Item::File("{}"));
    assert_eq!(target(&scenario, "/x/d", false), Ok("x/d.json".into()));
    scenario.write(
        "x.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/a", "repo": "r"}"#),
    );
    assert_eq!(target(&scenario, "/x/d", false), Err(Code::PathShadowed));
    assert_eq!(target(&scenario, "/x/c", false), Err(Code::PathShadowed));
}

#[test]
fn a_write_that_would_degrade_its_own_path_is_refused_and_its_twin_is_admitted() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/x.json", Item::File("{}"))
        .write("tasks/d.json", Item::File("{}"))
        .write("tasks/d/child.json", Item::File("{}"));
    let pointer = target(&scenario, "/tasks/d", true);
    assert_eq!(
        pointer,
        Err(Code::PathShadowed),
        "a pointer beside a directory"
    );
    assert_eq!(
        target(&scenario, "/tasks/d", false),
        Ok("tasks/d.json".into())
    );
    let case = target(&scenario, "/tasks/X", false);
    assert_eq!(case, Err(Code::PathCaseCollision), "beside tasks/x.json");
    assert_eq!(
        target(&scenario, "/tasks/y", false),
        Ok("tasks/y.json".into())
    );
    assert_eq!(
        target(&scenario, "/tasks/x/child", false),
        Ok("tasks/x/child.json".into()),
        "a directory beside a record that is not a pointer"
    );
}

#[test]
fn a_snapshot_lists_each_directory_once_and_each_target_without_one_lists_again() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{}"))
        .write("tasks/b.json", Item::File("{}"));
    let repositories = scenario.repositories();
    let repository = repositories.root();
    let mut snapshot = repositories.snapshot().unwrap();
    for text in ["/tasks/a", "/tasks/b", "/tasks/new"] {
        snapshot.target(&path(text), false).unwrap();
    }
    assert_eq!(repository.directories_listed(), 2, "the root and tasks");
    repositories.target(&path("/tasks/a"), false).unwrap();
    assert_eq!(repository.directories_listed(), 4);
}

#[test]
fn a_resolver_names_its_repository_when_debugged() {
    let scenario = Scenario::new();
    let repositories = scenario.repositories();
    let shown = format!("{:?}", repositories.snapshot().unwrap());
    let path = format!("{:?}", repositories.root().path());
    assert_eq!(shown, format!("Resolver {{ repository: {path}, .. }}"));
}

/// Whether resolving `/work/x` finds the blob of `{}` or a code, beside
/// several root files and one large one, and how many files it opens.
/// `None` when `x` is unreadable but a superuser reads it anyway.
fn resolve_beside_large_files(x: Item<'_>) -> Option<(Result<bool, Code>, usize)> {
    let locked = matches!(x, Item::Unreadable(_));
    let scenario = Scenario::new();
    let large = "a".repeat(1 << 20);
    scenario
        .write("large.bin", Item::File(&large))
        .write("a.md", Item::File("a"))
        .write("b.json", Item::File("{}"))
        .write("work/y.json", Item::File("{}"))
        .write("work/x.json", x);
    if locked && std::fs::File::open(scenario.workdir().join("work/x.json")).is_ok() {
        return None;
    }
    let repositories = scenario.repositories();
    let found = repositories.resolve(&Source::WorkingTree, &path("/work/x"));
    let empty = Object::Blob(scenario.blob("{}"));
    let found = found.map(|location| location.object == empty);
    Some((
        found.map_err(|error| error.code()),
        repositories.root().files_opened(),
    ))
}

#[test]
fn resolution_opens_only_the_file_it_picks_and_an_unreadable_one_is_still_unreadable() {
    let readable = resolve_beside_large_files(Item::File("{}"));
    // x.json is hashed when picked and then read to tell whether it is a
    // pointer; no other file is opened.
    assert_eq!(readable, Some((Ok(true), 2)));
    if let Some(unreadable) = resolve_beside_large_files(Item::Unreadable("{}")) {
        assert_eq!(unreadable, (Err(Code::RepoUnreadable), 1), "x.json alone");
    }
}

#[test]
fn a_record_beside_its_children_is_opened_once_however_many_children_a_resolver_reaches() {
    let scenario = Scenario::new();
    scenario
        .write("p.json", Item::File(r#"{"p": 0}"#))
        .write("p/a.json", Item::File(r#"{"a": 1}"#))
        .write("p/b.json", Item::File(r#"{"b": 2}"#))
        .write("p/c.json", Item::File(r#"{"c": 3}"#));
    let repositories = scenario.repositories();
    let mut snapshot = repositories.snapshot().unwrap();
    let opened: Vec<usize> = ["/p/a", "/p/b", "/p/c", "/p/a", "/p/b"]
        .into_iter()
        .map(|text| {
            snapshot.resolve(&path(text)).unwrap();
            repositories.root().files_opened()
        })
        .collect();
    // p.json is hashed and read once to tell that it does not shadow p/.
    // Each child is hashed and read for a pointer the first time it is
    // reached, and not opened again.
    assert_eq!(opened, [4, 6, 8, 8, 8]);
}

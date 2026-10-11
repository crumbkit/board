//! The working-tree source: every file git sees (docs/architecture.md,
//! Boards and entries).

use std::path::{Path, PathBuf};

use super::*;
use crate::tree::{Kind, Listed, RawEntry, Visit};
use crate::worktree::WorkingTree as WorkingTreeView;
use crate::{Contents, Entry, GitError};

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

#[cfg(unix)]
#[test]
fn a_socket_is_never_an_entry_while_the_file_beside_it_is() {
    let scenario = Scenario::new();
    scenario.write("x.json", Item::File("{}"));
    let socket = scenario.workdir().join("x.sock");
    let _listening = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    assert_eq!(walk(&scenario, WorkingTree), [ok("x.json", "/x")]);
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
    assert!(
        entries[0].stat.is_some(),
        "the stat it was seen with is kept"
    );
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
            &|path, contents| (path.to_owned(), contents),
            &mut |kept_one| {
                kept.push(kept_one);
                Ok::<(), GitError>(())
            },
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

/// Walks `scenario`'s working tree hashing on `threads` threads, with no
/// blob known: the entries, the files kept in path order, and how many files
/// were opened.
fn walked_on(scenario: &Scenario, threads: usize) -> (Vec<Entry>, Vec<(String, Contents)>, usize) {
    let repository = scenario.open();
    repository.set_threads(std::num::NonZeroUsize::new(threads).unwrap());
    let mut kept = Vec::new();
    let entries = repository
        .walk_working_tree(
            &mut |_, _| None,
            &|path, contents| (path.to_owned(), contents),
            &mut |kept_one| {
                kept.push(kept_one);
                Ok::<(), GitError>(())
            },
        )
        .unwrap();
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    (entries, kept, repository.files_opened())
}

#[test]
fn a_walk_on_one_thread_and_on_four_give_the_same_entries_keep_the_same_files_and_open_each_once() {
    let scenario = Scenario::new();
    for n in 0..40 {
        let text = format!("{{\"n\": {n}}}");
        scenario.write(&format!("d{}/r{n}.json", n % 5), Item::File(&text));
    }
    scenario
        .write("notes.md", Item::File("not kept"))
        .write("locked.json", Item::Unreadable("{}"))
        .write("link.json", Item::Symlink("d0/r0.json"));
    let one = walked_on(&scenario, 1);
    let four = walked_on(&scenario, 4);
    assert_eq!(one, four);
    let (entries, kept, opened) = one;
    assert_eq!(entries.len(), 43);
    assert_eq!(kept.len(), 40, "every readable JSON file is kept");
    assert_eq!(opened, 42, "every file but the symlink is opened once");
    let locked = entries
        .iter()
        .find(|entry| entry.path == "locked.json")
        .unwrap();
    assert_eq!((locked.blob, locked.stat.is_some()), (None, true));
    assert_eq!(
        entries.iter().filter(|entry| entry.blob.is_some()).count(),
        42
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn a_walk_gives_the_same_entries_however_many_threads_list_and_hash(files in checkout()) {
        prop_assume!(files.iter().all(|file| !files.iter().any(|other| other.starts_with(&format!("{file}/")))));
        let scenario = Scenario::new();
        for file in &files {
            scenario.write(file, Item::File("{}"));
        }
        let one = walked_on(&scenario, 1);
        for threads in 2..=4 {
            prop_assert_eq!(&walked_on(&scenario, threads), &one, "{} threads", threads);
        }
    }
}

#[test]
fn an_ignored_directory_is_never_listed_however_many_threads_list() {
    let scenario = Scenario::new();
    scenario
        .write(".gitignore", Item::File("build/\n"))
        .write("src/a.json", Item::File("{}"))
        .write("build/x/y.json", Item::File("{}"))
        .write("build/z.json", Item::File("{}"));
    for threads in [1, 4] {
        let repository = scenario.open();
        repository.set_threads(std::num::NonZeroUsize::new(threads).unwrap());
        let entries = repository
            .walk_working_tree(&mut |_, _| None, &|_, _| {}, &mut |()| {
                Ok::<(), GitError>(())
            })
            .unwrap();
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, [".gitignore", "src/a.json"], "{threads} threads");
        assert_eq!(
            repository.directories_listed(),
            2,
            "the root and src on {threads} threads"
        );
    }
}

#[test]
fn a_repository_opens_with_as_many_threads_as_the_machine_has_cores_up_to_four() {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    assert_eq!(Scenario::new().open().threads(), cores.min(4));
}

#[test]
fn a_record_beside_its_directory_is_hashed_once_while_addressing_and_the_rest_on_the_threads() {
    let scenario = Scenario::new();
    scenario
        .write("tasks/a.json", Item::File("{\"beside\": true}"))
        .write("tasks/a/b.json", Item::File("{\"under\": true}"))
        .write("tasks/c.json", Item::File("{\"beside\": false}"));
    let one = walked_on(&scenario, 1);
    let four = walked_on(&scenario, 4);
    assert_eq!(one, four);
    let (entries, kept, opened) = one;
    assert_eq!(entries.len(), 3);
    let paths: Vec<_> = kept.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, ["tasks/a.json", "tasks/a/b.json", "tasks/c.json"]);
    // Addressing tasks/ hashes a.json on this thread to read whether it
    // shadows a/, and reads it once more; settling finds it hashed already.
    assert_eq!(opened, 4, "three files hashed once each, and one read once");
}

/// Every directory a visit lists, by name: its entries, or the code of
/// the error listing it gave.
type Listings = Vec<(PathBuf, Result<Vec<RawEntry<PathBuf>>, Code>)>;

/// Lists `board` on a fresh working tree, `ahead` on four threads or else
/// on this one, and returns what was listed with how many directories were
/// read from disk.
fn listed_by(board: &Scenario, ahead: bool) -> (Listings, usize) {
    let repository = board.open();
    repository.set_threads(std::num::NonZeroUsize::new(4).unwrap());
    let mut tree = repository.working_tree(None).unwrap();
    let mut listings = Listings::new();
    let start = (PathBuf::new(), ());
    let visit: Visit<'_, WorkingTreeView<'_, '_>, ()> = &mut |_, (directory, ()), listed| {
        let entries = match listed {
            Ok(entries) => entries,
            Err(error) => {
                listings.push((directory, Err(error.code())));
                return Ok(Vec::new());
            }
        };
        let children = entries
            .iter()
            .filter_map(|entry| match &entry.kind {
                Kind::Directory(child) => Some((child.clone(), ())),
                _ => None,
            })
            .collect();
        listings.push((directory, Ok(entries.to_vec())));
        Ok(children)
    };
    if ahead {
        tree.visit_ahead(start, visit).unwrap();
    } else {
        crate::tree::visit_each(&mut tree, start, visit).unwrap();
    }
    listings.sort_by(|a, b| a.0.cmp(&b.0));
    (listings, repository.directories_listed())
}

#[test]
fn listing_the_seed_board_ahead_on_four_threads_gives_what_listing_it_on_this_thread_gives() {
    let seed = board_scenario::seed::Seed::new();
    seed.board
        .write(
            ".gitignore",
            Item::File(
                "ignored/
",
            ),
        )
        .write("ignored/skipped.json", Item::File("{}"))
        .write("locked", Item::Locked);
    let each = listed_by(&seed.board, false);
    let ahead = listed_by(&seed.board, true);
    assert_eq!(each, ahead);
    let (listings, _) = each;
    let directories: Vec<&Path> = listings
        .iter()
        .map(|(directory, _)| directory.as_path())
        .collect();
    assert!(
        !directories.contains(&Path::new("ignored")),
        "an ignored directory is never listed"
    );
    assert!(
        !directories.contains(&Path::new("repo/nested")),
        "a nested repository is never listed"
    );
    let locked = listings
        .iter()
        .find(|(directory, _)| directory == Path::new("locked"));
    assert!(
        matches!(locked, Some((_, Err(Code::RepoUnreadable)))),
        "a locked directory is listed as unreadable: {locked:?}"
    );
    assert!(directories.contains(&Path::new("mount")));
}

/// What a walk's visit of one directory answers in a test.
type Visited = Result<Vec<(PathBuf, ())>, GitError>;

/// Runs `visit` over a small board on another thread, with four listing
/// threads, and returns how that thread ended; it fails if the walk does
/// not end soon.
fn visited_on_another_thread(
    visit: fn(&mut WorkingTreeView<'_, '_>, (PathBuf, ()), Listed<PathBuf>) -> Visited,
) -> std::thread::Result<Result<(), GitError>> {
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let walker = std::thread::spawn(move || {
        let _done = done;
        let scenario = Scenario::new();
        scenario
            .write("a/x.json", Item::File("{}"))
            .write("b/y.json", Item::File("{}"));
        let repository = scenario.open();
        repository.set_threads(std::num::NonZeroUsize::new(4).unwrap());
        let mut tree = repository.working_tree(None).unwrap();
        let mut visit = visit;
        tree.visit_ahead((PathBuf::new(), ()), &mut visit)
    });
    assert_eq!(
        finished.recv_timeout(std::time::Duration::from_secs(10)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected),
        "the walk ends while the listing threads wait for directories"
    );
    walker.join()
}

#[test]
fn a_visit_that_returns_ends_the_walk_and_one_that_panics_ends_it_with_its_panic() {
    let returned = visited_on_another_thread(|_, _, listed| {
        Ok(listed?
            .iter()
            .filter_map(|entry| match &entry.kind {
                Kind::Directory(child) => Some((child.clone(), ())),
                _ => None,
            })
            .collect())
    });
    assert!(matches!(returned, Ok(Ok(()))));
    let panicked = visited_on_another_thread(|_, _, _| panic!("the visit stops"));
    let payload = panicked.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"the visit stops"));
}

/// A directory listed by a stand-in listing thread: the listing, or the
/// payload of the panic that listing it raised.
type StoodIn<R> = (PathBuf, (), std::thread::Result<std::io::Result<Vec<R>>>);

/// Answers each directory sent to `take` with an empty listing, except
/// `boom`, which it answers with a caught panic's payload, as a listing
/// thread whose `read` panicked would.
fn list_or_panic<R>(
    take: &std::sync::Mutex<std::sync::mpsc::Receiver<(PathBuf, ())>>,
    send: &std::sync::mpsc::Sender<StoodIn<R>>,
) {
    loop {
        let received = take.lock().unwrap().recv();
        let Ok((directory, payload)) = received else {
            return;
        };
        let listed = if directory == Path::new("boom") {
            Err(Box::new("the listing panics") as Box<dyn std::any::Any + Send>)
        } else {
            Ok(Ok(Vec::new()))
        };
        if send.send((directory, payload, listed)).is_err() {
            return;
        }
    }
}

#[test]
fn a_listing_that_arrives_as_a_panic_ends_the_walk_with_that_panic_while_another_thread_lists() {
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let walker = std::thread::spawn(move || {
        let _done = done;
        let scenario = Scenario::new();
        scenario.write("a/x.json", Item::File("{}"));
        let repository = scenario.open();
        let mut tree = repository.working_tree(None).unwrap();
        let (to_list, take) = std::sync::mpsc::channel();
        let (send, listed) = std::sync::mpsc::channel();
        let take = &std::sync::Mutex::new(take);
        let mut visit = |_: &mut WorkingTreeView<'_, '_>,
                         (directory, ()): (PathBuf, ()),
                         listed: Listed<PathBuf>| {
            listed?;
            Ok(if directory.as_os_str().is_empty() {
                vec![(PathBuf::from("a"), ()), (PathBuf::from("boom"), ())]
            } else {
                vec![]
            })
        };
        std::thread::scope(|scope| {
            let to_list = to_list;
            let send = send;
            for _ in 0..2 {
                let send = send.clone();
                scope.spawn(move || list_or_panic(take, &send));
            }
            drop(send);
            tree.visit_received((PathBuf::new(), ()), &mut visit, &to_list, &listed)
        })
    });
    assert_eq!(
        finished.recv_timeout(std::time::Duration::from_secs(10)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected),
        "the walk ends while the other listing thread waits for directories"
    );
    let payload = walker.join().unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"the listing panics"));
}

#[test]
fn a_repository_hashes_on_up_to_four_threads_unless_it_is_set_to_another_number() {
    let scenario = Scenario::new();
    let repository = scenario.open();
    assert!((1..=4).contains(&repository.threads()));
    repository.set_threads(std::num::NonZeroUsize::new(7).unwrap());
    assert_eq!(repository.threads(), 7);
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
        Err(crate::WriteError::Degraded { code, .. }) => assert_eq!(code, Code::PathShadowed),
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

/// A directory holding a file, an ignored file, two symlinks and a
/// directory, beside other files.
fn lazily_listed() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(".gitignore", Item::File("d/ignored.json\n"))
        .write("d/a.json", Item::File("{}"))
        .write("d/b.json", Item::File("{}"))
        .write("d/x.json", Item::File(r#"{"x": 1}"#))
        .write("d/ignored.json", Item::File("{}"))
        .write("d/link", Item::Symlink("x.json"))
        .write("d/s.json", Item::Symlink("x.json"))
        .write("d/sub/z.json", Item::File("{}"));
    scenario
}

#[test]
fn a_resolver_classifies_only_the_entries_a_path_names_in_a_directory() {
    let scenario = lazily_listed();
    let repositories = scenario.repositories();
    let mut asked = Vec::new();
    let mut known = |path: &str, _: &board_model::Stat| {
        asked.push(path.to_owned());
        None
    };
    let mut resolver = repositories.resolver(&mut known).unwrap();
    let found = resolver.resolve(&path("/d/x")).unwrap();
    let missing = resolver.resolve(&path("/d/missing")).unwrap_err();
    let again = resolver.resolve(&path("/d/x")).unwrap();
    drop(resolver);
    assert_eq!(found.path, "d/x.json");
    assert_eq!(again, found);
    assert_eq!(missing.code(), Code::PathNotFound);
    // Only d/x.json is looked up, once; its neighbours are never classified.
    assert_eq!(asked, ["d/x.json"]);
    assert_eq!(
        repositories.root().directories_listed(),
        2,
        "the root and d"
    );
}

#[cfg(unix)]
#[test]
fn a_resolver_answers_the_same_through_a_lazy_listing_as_through_a_full_one() {
    use crate::address::Pointers;
    use crate::tree::Tree;
    use crate::walk::{Planned, planned_code};
    let scenario = lazily_listed();
    let repositories = scenario.repositories();
    let tree = |full: bool| {
        let mut tree = repositories.root().working_tree(None).unwrap().on_demand();
        if full {
            tree.list(&PathBuf::from("d")).unwrap();
        }
        tree
    };
    let resolve = |full: bool, text: &str| {
        let mut pointers = Pointers::default();
        repositories
            .resolve_tree(&mut tree(full), &path(text), true, &mut pointers)
            .map(|location| (location.path, location.object))
            .map_err(|error| error.code())
    };
    let symlink = Object::Blob(scenario.blob("x.json"));
    let expected = [
        (
            "/d/x",
            Ok((
                "d/x.json".to_owned(),
                Object::Blob(scenario.blob(r#"{"x": 1}"#)),
            )),
        ),
        ("/d/missing", Err(Code::PathNotFound)),
        ("/d/ignored", Err(Code::PathNotFound)),
        ("/d/X", Err(Code::PathNotFound)),
        ("/d/link", Ok(("d/link".to_owned(), symlink))),
        ("/d/s.json", Ok(("d/s.json".to_owned(), symlink))),
        ("/d/s", Err(Code::PathNotFound)),
        (
            "/d/sub/z",
            Ok(("d/sub/z.json".to_owned(), Object::Blob(scenario.blob("{}")))),
        ),
    ];
    for (text, answer) in expected {
        assert_eq!(resolve(false, text), answer, "{text}, lazily");
        assert_eq!(resolve(true, text), answer, "{text}, fully listed");
    }
    let planned = |full: bool, file: &str| planned_code(&mut tree(full), file, false).unwrap();
    for (file, code) in [
        ("d/X.json", Some(Planned::File(Code::PathCaseCollision))),
        ("d/new.json", None),
    ] {
        assert_eq!(planned(false, file), code, "{file}, lazily");
        assert_eq!(planned(true, file), code, "{file}, fully listed");
    }
}

//! The working tree's decision, which the index keeps in a slot with the
//! key of all it depends on. A warm query or check of an unchanged board
//! decides no entry, and after each change decides again only the entries
//! the change could alter, and answers as a cold one.

use std::path::Path;

use board_index::Index;
use board_scenario::{Item, Scenario};

use super::decisions::query_as;
use super::{Need, prune};
use crate::tests::{Library, board, open, task};

/// A change to make between two queries, and what it is.
type Change<'s> = (&'static str, Box<dyn Fn() + 's>);

/// How many entries a warm query of `scenario` through the index in
/// `cache` decided, after it answered as a cold one.
fn decided(scenario: &Scenario, cache: &Path) -> usize {
    query_as(scenario, cache, "test").decided
}

/// A board that mounts the library, with tasks that refer into it and into
/// the board, an ignore rule and a file it ignores.
fn mounting() -> (Scenario, Library) {
    let scenario = board();
    let vendor = Library::mounted(&scenario);
    scenario
        .write(".gitignore", Item::File("ignored.json\n"))
        .write("ignored.json", Item::File(&task("")))
        .write(
            "tasks/a.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write("tasks/b.json", Item::File(&task(r#", "see": "/tasks/a""#)));
    (scenario, vendor)
}

#[test]
fn a_warm_query_of_an_unchanged_board_decides_no_entry_and_a_cold_one_decides_each() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    assert!(decided(&scenario, cache.path()) > 0, "a cold index decides");
    assert_eq!(decided(&scenario, cache.path()), 0);
}

#[test]
fn a_build_that_names_no_build_decides_every_entry_each_time_and_its_named_twin_keeps_them() {
    for (build, keeps) in [("", false), ("test", true)] {
        let (scenario, _vendor) = mounting();
        let cache = tempfile::tempdir().unwrap();
        query_as(&scenario, cache.path(), build);
        let warm = query_as(&scenario, cache.path(), build);
        assert_eq!(warm.decided == 0, keeps, "{build:?}");
    }
}

/// A note that links to three directories, then those directories made
/// empty, removed and hidden by a rule in `.git/info/exclude`, which no
/// entry holds: a resolution reaches a directory that holds no entry. Each
/// step leaves a set of directories no earlier step left, as a set seen
/// before keeps its decision.
fn directories(scenario: &Scenario) -> Vec<Change<'_>> {
    let made = |name: &str| {
        let file = format!("topics/{name}/x.json");
        scenario
            .write(&file, Item::File(&task("")))
            .write(&file, Item::Deleted);
    };
    vec![
        (
            "a note that links to directories not made yet",
            Box::new(|| {
                let note = "[a](/topics/a) [b](/topics/b) [c](/topics/c)\n";
                scenario.write("notes/n.md", Item::File(note));
            }),
        ),
        ("an empty directory made", Box::new(move || made("a"))),
        ("a second empty directory made", Box::new(move || made("b"))),
        (
            "an empty directory removed",
            Box::new(|| {
                scenario.write("topics/a", Item::Deleted);
            }),
        ),
        (
            "an empty directory made and another hidden by an exclude rule outside the tree",
            Box::new(move || {
                made("c");
                scenario.write(".git/info/exclude", Item::File("topics/b/\n"));
            }),
        ),
    ]
}

#[test]
fn after_each_change_a_warm_query_decides_again_and_answers_as_a_cold_one() {
    let (scenario, vendor) = mounting();
    // Enough other entries that most changes decide only what they reach.
    fill(&scenario, 80);
    let cache = tempfile::tempdir().unwrap();
    let pruned = || {
        let board = open(&scenario);
        prune(Index::open(Some(cache.path()), board.repository().path()).unwrap());
    };
    let mut changes: Vec<Change<'_>> = vec![
        (
            "an edit in place of the same size",
            Box::new(|| {
                scenario.write(
                    "tasks/a.json",
                    Item::File(&task(r#", "project": "/projects/q""#)),
                );
            }),
        ),
        (
            "a rename",
            Box::new(|| {
                let text = task(r#", "see": "/tasks/a""#);
                scenario
                    .write("tasks/c.json", Item::File(&text))
                    .write("tasks/b.json", Item::Deleted);
            }),
        ),
        (
            "a delete",
            Box::new(|| {
                scenario.write("tasks/c.json", Item::Deleted);
            }),
        ),
        (
            "a commit that moves HEAD",
            Box::new(|| {
                scenario.commit(&[("projects/p.json", r#"{"$type": "lr/project/v1"}"#)]);
            }),
        ),
        (
            "an ignore rule",
            Box::new(|| {
                scenario.write(".gitignore", Item::File("ignored.json\ntasks/a.json\n"));
            }),
        ),
        (
            "an ignored file added to git's index",
            Box::new(|| {
                let commit = scenario.commit(&[("ignored.json", &task(""))]);
                scenario.track(commit);
            }),
        ),
        (
            "a sibling whose name differs only in case",
            Box::new(|| {
                // A file system that folds case writes over projects/p.json.
                let project = r#"{"$type": "lr/project/v1", "n": 1}"#;
                scenario.write("Projects/P.json", Item::File(project));
            }),
        ),
        (
            "a schema change",
            Box::new(|| {
                let schema = r#"{"type": "object", "required": ["title", "due"]}"#;
                scenario.write(".board/types/lr/task/v1.schema.json", Item::File(schema));
            }),
        ),
        ("a mount moved", Box::new(|| vendor.moved(&scenario))),
        ("a prune between runs", Box::new(pruned)),
        (
            "a file dated after the clock",
            Box::new(|| {
                scenario.write("tasks/d.json", Item::Ahead(&task("")));
            }),
        ),
    ];
    changes.extend(directories(&scenario));
    changes.extend(more(&scenario));
    assert!(decided(&scenario, cache.path()) > 0);
    assert_eq!(decided(&scenario, cache.path()), 0, "nothing changed");
    for (change, make) in changes {
        make();
        let count = decided(&scenario, cache.path());
        // A commit changes no entry, so no entry is decided again; only
        // each row's `change` is set again. No entry refers to the file
        // deleted.
        let none = ["a commit that moves HEAD", "a delete"].contains(&change);
        assert_eq!(count == 0, none, "{change}: {count}");
        assert_eq!(
            decided(&scenario, cache.path()),
            0,
            "{change}, then nothing"
        );
    }
}

/// Changes to repo records, a symlink and links through it, each answered
/// as a cold run answers it.
fn more(scenario: &Scenario) -> Vec<Change<'_>> {
    let repo = |remote: &str| format!(r#"{{"$type": "board/repo/v1", "remote": "{remote}"}}"#);
    vec![
        (
            "a repo record",
            Box::new(move || {
                scenario.write("repos/a.json", Item::File(&repo("example-org/a")));
            }),
        ),
        (
            "a second repo record that names the first's remote",
            Box::new(move || {
                scenario.write("repos/b.json", Item::File(&repo("example-org/a")));
            }),
        ),
        (
            "a symlink and a note that links through it",
            Box::new(|| {
                scenario
                    .write("linked/x.json", Item::Symlink("../projects/p.json"))
                    .write("notes/l.md", Item::File("[x](/linked/x)\n"));
            }),
        ),
        (
            "the symlink retargeted",
            Box::new(|| {
                scenario
                    .write("linked/x.json", Item::Deleted)
                    .write("linked/x.json", Item::Symlink("../tasks/a.json"));
            }),
        ),
    ]
}

/// Writes `count` tasks in `topics/` that refer to nothing.
fn fill(scenario: &Scenario, count: usize) {
    for at in 0..count {
        let fields = format!(r#", "note": "{at}""#);
        scenario.write(&format!("topics/t{at}.json"), Item::File(&task(&fields)));
    }
}

/// `mounting`'s board with `count` more tasks in `topics/`, which refer to
/// nothing, and a task whose reference to `/projects/z` dangles.
fn crowded(count: usize) -> (Scenario, Library) {
    let (scenario, vendor) = mounting();
    fill(&scenario, count);
    let dangling = task(r#", "see": "/projects/z""#);
    scenario.write("tasks/z.json", Item::File(&dangling));
    (scenario, vendor)
}

#[test]
fn after_a_record_is_added_a_warm_query_decides_it_and_each_entry_whose_reference_reaches_it() {
    let (scenario, _vendor) = crowded(60);
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    assert_eq!(decided(&scenario, cache.path()), 0);
    let project = r#"{"$type": "lr/project/v1"}"#;
    scenario.write("projects/q.json", Item::File(project));
    assert_eq!(
        decided(&scenario, cache.path()),
        1,
        "a record nothing refers to"
    );
    scenario.write("projects/z.json", Item::File(project));
    assert_eq!(
        decided(&scenario, cache.path()),
        2,
        "a record a task refers to"
    );
}

#[test]
fn a_commit_that_changes_no_entry_decides_none_and_sets_each_rows_change_as_a_cold_run_does() {
    let (scenario, _vendor) = crowded(60);
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    let committed = task(r#", "project": "/projects/p""#);
    scenario.commit(&[("tasks/a.json", &committed)]);
    assert_eq!(decided(&scenario, cache.path()), 0);
    scenario.commit(&[("tasks/b.json", &task(r#", "see": "/tasks/a""#))]);
    assert_eq!(decided(&scenario, cache.path()), 0, "a second commit");
}

#[test]
fn more_than_one_in_twenty_entries_changed_decides_every_entry_and_fewer_decide_those_changed() {
    for (changed, every) in [(1, false), (10, true)] {
        let (scenario, _vendor) = crowded(60);
        let cache = tempfile::tempdir().unwrap();
        decided(&scenario, cache.path());
        for at in 0..changed {
            let fields = format!(r#", "note": "changed {at}""#);
            scenario.write(&format!("topics/t{at}.json"), Item::File(&task(&fields)));
        }
        let count = decided(&scenario, cache.path());
        let expected = if every { count > 60 } else { count == changed };
        assert!(expected, "{changed} changed decided {count}");
    }
}

#[test]
fn a_decision_whose_write_rolled_back_is_not_kept_and_the_next_query_decides_again() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let writer = index.write().unwrap();
    board.decide(&writer, None, Need::View).unwrap();
    writer.rollback().unwrap();
    assert!(decided(&scenario, cache.path()) > 0);
    assert_eq!(decided(&scenario, cache.path()), 0);
}

#[test]
fn a_warm_check_and_a_warm_query_of_an_unchanged_board_read_no_validation() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    let board = open(&scenario);
    for need in [Need::View, Need::Check] {
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let (read, _, _) = crate::index::transact(index, |writer| {
            let (_, entries) = board.decide(writer, None, need)?;
            Ok((writer.validations_read(), entries))
        })
        .unwrap();
        assert_eq!(read, 0, "{need:?}");
    }
}

#[test]
fn after_one_record_changes_a_warm_query_reads_only_its_validation_and_a_cold_one_reads_each() {
    let (scenario, _vendor) = crowded(60);
    let cache = tempfile::tempdir().unwrap();
    decided(&scenario, cache.path());
    let changed = task(r#", "note": "changed""#);
    scenario.write("topics/t0.json", Item::File(&changed));
    let board = open(&scenario);
    for (cache, most) in [(Some(cache.path()), 1), (None, usize::MAX)] {
        let index = Index::open(cache, board.repository().path()).unwrap();
        let (read, _, _) = crate::index::transact(index, |writer| {
            let (_, entries) = board.decide(writer, None, Need::View)?;
            Ok((writer.validations_read(), entries))
        })
        .unwrap();
        let expected = if cache.is_some() {
            read == most
        } else {
            read > 60
        };
        assert!(expected, "{cache:?} read {read}");
    }
}

#[test]
fn a_warm_check_and_a_warm_query_of_an_unchanged_board_write_nothing_and_a_cold_one_writes() {
    let (scenario, _vendor) = mounting();
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let wrote = |need| {
        let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
        let (wrote, _, _) = crate::index::transact(index, |writer| {
            let (_, entries) = board.decide(writer, None, need)?;
            Ok((writer.wrote()?, entries))
        })
        .unwrap();
        wrote
    };
    assert!(wrote(Need::View), "a cold query writes");
    for need in [Need::View, Need::Check] {
        assert!(!wrote(need), "{need:?}");
    }
}

/// The origin of the board `twinned` makes.
const ORIGIN: &str = "https://github.com/example-org/r.git";

/// The origin of the library `twinned` mounts when it is no cycle.
const TWIN: &str = "https://github.com/example-org/twin.git";

/// An origin no repository of the test is cloned from.
const OTHER: &str = "https://github.com/example-org/other.git";

/// Sets `scenario`'s `origin` to `url`, as `git remote set-url` does.
fn originating(scenario: &Scenario, url: &str) {
    let config = scenario.workdir().join(".git/config");
    let text = std::fs::read_to_string(&config).unwrap();
    let kept: String = (text.split_inclusive('\n'))
        .take_while(|line| !line.starts_with("[remote \"origin\"]"))
        .collect();
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    scenario.write(".git/config", Item::File(&format!("{kept}{remote}")));
}

/// `crowded(80)` with `/twin` mounting a library board whose origin is
/// `origin`, and a task that refers into it. The board's own origin is
/// [`ORIGIN`], so a library with that origin is a cycle.
fn twinned(origin: &str) -> (Scenario, Library, Scenario) {
    let (scenario, vendor) = crowded(80);
    originating(&scenario, ORIGIN);
    let twin = Scenario::new();
    originating(&twin, origin);
    let commit = twin.commit(&[
        (
            ".board/types/lr/project/v1.schema.json",
            crate::tests::PROJECT_SCHEMA,
        ),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ]);
    let pointer = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
        twin.path()
    );
    scenario.write("twin.json", Item::File(&pointer)).write(
        "tasks/twin.json",
        Item::File(&task(r#", "project": "/twin/projects/p""#)),
    );
    (scenario, vendor, twin)
}

/// Replaces the repository of `into` with a copy of `from`'s, at the same
/// path.
fn replaced(into: &Scenario, from: &Scenario) {
    into.write(".git", Item::Deleted);
    let mut pending = vec![from.workdir().join(".git")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
                continue;
            }
            let at = path.strip_prefix(from.workdir()).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            into.write(at.to_str().unwrap(), Item::Bytes(&bytes));
        }
    }
}

/// A change to the mounted library alone: what it is, the library's origin
/// before it, how to make it, and how many entries the next warm query
/// decides.
type Alone = (&'static str, &'static str, fn(&Scenario), usize);

/// A clone of `twin` whose origin is `url`.
fn cloned(twin: &Scenario, url: &str) -> Scenario {
    let clone = Scenario::clone_of(twin);
    originating(&clone, url);
    clone
}

#[test]
fn after_each_change_to_a_mounted_repository_alone_a_warm_query_decides_its_pointer_and_what_reaches_it()
 {
    // Each change leaves every pointer file as it was. The pointer
    // `twin.json` and the task that refers through it are decided when the
    // mount's answer changes, and a mount that resolves adds the two
    // entries of the library it walks. A repository of another origin that
    // holds the same commit answers as the one it replaced.
    let changes: [Alone; 4] = [
        (
            "the mounted repository removed",
            TWIN,
            |twin| {
                twin.write("", Item::Deleted);
            },
            2,
        ),
        (
            "the mounted repository replaced at the same path by one with another origin",
            TWIN,
            |twin| replaced(twin, &cloned(twin, OTHER)),
            0,
        ),
        (
            "the mounted repository replaced at the same path by one with the board's origin",
            TWIN,
            |twin| replaced(twin, &cloned(twin, ORIGIN)),
            2,
        ),
        (
            "a cycle that becomes a resolved mount",
            ORIGIN,
            |twin| {
                originating(twin, OTHER);
            },
            4,
        ),
    ];
    for (change, origin, make, expected) in changes {
        let (scenario, _vendor, twin) = twinned(origin);
        let cache = tempfile::tempdir().unwrap();
        decided(&scenario, cache.path());
        assert_eq!(
            decided(&scenario, cache.path()),
            0,
            "{change}: nothing changed"
        );
        make(&twin);
        assert_eq!(decided(&scenario, cache.path()), expected, "{change}");
        assert_eq!(
            decided(&scenario, cache.path()),
            0,
            "{change}, then nothing"
        );
    }
}

/// A board with a task beside a directory of tasks, and a note that links
/// to it.
fn beside() -> Scenario {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/a/b.json", Item::File(&task(r#", "note": "b""#)))
        .write("notes/n.md", Item::File("[a](/tasks/a)\n"));
    scenario
}

/// How many files a run deciding `scenario` through the index in `cache`
/// opened, and how many blob facts it looked up one by one.
fn reads(scenario: &Scenario, cache: &Path) -> (usize, usize) {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let before = board.repository().files_opened();
    let (looked_up, _, _) = crate::index::transact(index, |writer| {
        let (_, entries) = board.decide(writer, None, Need::Report)?;
        Ok((writer.blobs_read(), entries))
    })
    .unwrap();
    (board.repository().files_opened() - before, looked_up)
}

#[test]
fn a_warm_run_of_an_unchanged_board_reads_no_record_for_a_pointer_and_looks_up_no_blob() {
    let scenario = beside();
    let cache = tempfile::tempdir().unwrap();
    let (opened, _) = reads(&scenario, cache.path());
    assert!(opened > 0, "a cold run opens each file");
    assert_eq!(reads(&scenario, cache.path()), (0, 0), "warm");
}

#[test]
fn after_a_record_beside_its_directory_becomes_a_pointer_a_warm_run_hides_the_directory() {
    let scenario = beside();
    let cache = tempfile::tempdir().unwrap();
    query_as(&scenario, cache.path(), "test");
    let pointer = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    scenario.write("tasks/a.json", Item::File(pointer));
    query_as(&scenario, cache.path(), "test");
    let report = open(&scenario).check(None, Some(cache.path())).unwrap();
    let hidden = report
        .entries
        .iter()
        .find(|entry| entry.path == "tasks/a/b.json");
    assert_eq!(
        hidden.unwrap().status,
        board_tree::Status::Degraded(board_model::Code::PathShadowed)
    );
}

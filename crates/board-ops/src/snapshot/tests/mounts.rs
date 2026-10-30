//! Checking boards reachable through mounts.

use board_model::{Change, Code, ObjectId, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};
use crate::{CheckReport, Checked};

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A library board, committed: one valid task, one whose reference dangles
/// in its own board, and its own copy of the types.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/ok.json", &task(r#", "project": "/projects/p""#)),
        ("tasks/dangling.json", &task(r#", "see": "/nowhere""#)),
    ]);
    (library, commit)
}

fn mounted(report: &CheckReport) -> Vec<(String, Status)> {
    report
        .entries
        .iter()
        .filter(|entry| matches!(entry.source, Source::Commit(_)))
        .map(|entry| (entry.logical.as_ref().unwrap().to_string(), entry.status))
        .collect()
}

fn check(scenario: &Scenario, scope: Option<&str>) -> CheckReport {
    let scope = scope.map(|scope| scope.parse().unwrap());
    open(scenario).check(scope.as_ref(), None).unwrap()
}

#[test]
fn a_mounted_board_is_checked_in_itself_and_named_under_its_mount_point() {
    let (library, commit) = library();
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    let report = check(&scenario, None);
    assert_eq!(
        mounted(&report),
        [
            (
                "/vendor/lib/.board/types/lr/project/v1.schema".to_owned(),
                Status::Ok
            ),
            (
                "/vendor/lib/.board/types/lr/task/v1.schema".to_owned(),
                Status::Ok
            ),
            ("/vendor/lib/projects/p".to_owned(), Status::Ok),
            (
                "/vendor/lib/tasks/dangling".to_owned(),
                Status::Degraded(Code::RefDangling)
            ),
            ("/vendor/lib/tasks/ok".to_owned(), Status::Ok),
        ]
    );
    let ok = report
        .entries
        .iter()
        .find(|entry| entry.path == "tasks/ok.json")
        .unwrap();
    assert_eq!(
        (&ok.source, ok.change),
        (&Source::Commit(commit), Change::Committed)
    );
}

#[test]
fn a_board_mounted_twice_is_indexed_once_under_its_first_route_and_in_scope_on_either() {
    let (library, commit) = library();
    let scenario = board();
    let to_library = mount(&library.path(), commit);
    scenario
        .write("b.json", Item::File(&to_library))
        .write("a.json", Item::File(&to_library));
    let report = check(&scenario, None);
    let entries = mounted(&report);
    assert_eq!(entries.len(), 5, "one row per entry, not per mount point");
    assert!(
        entries
            .iter()
            .all(|(logical, _)| logical.starts_with("/a/"))
    );
    assert_eq!(mounted(&check(&scenario, Some("/b"))), entries);
    assert_eq!(mounted(&check(&scenario, Some("/b/tasks"))).len(), 2);
    assert_eq!(mounted(&check(&scenario, Some("/a/tasks"))).len(), 2);
    assert_eq!(mounted(&check(&scenario, Some("/projects"))), []);
}

#[test]
fn a_check_opens_a_repository_two_mounts_name_once_and_counts_it_and_the_pointers_each_source_read()
{
    let (library, commit) = library();
    let scenario = board();
    let to_library = mount(&library.path(), commit);
    scenario
        .write("b.json", Item::File(&to_library))
        .write("a.json", Item::File(&to_library));
    let board = open(&scenario);
    let work = board.check(None, None).unwrap().work;
    assert_eq!(work.repositories, 1);
    let trace = board.take_trace().work;
    assert_eq!(trace.get("repositories"), Some(&1));
    assert_eq!(trace.get("pointers_read"), Some(&work.pointers_read));
    // The pointer in the working tree. The project the mounted task refers
    // to is not read, because its facts say it holds no pointer.
    assert_eq!(work.pointers_read, 1);
}

#[test]
fn a_check_scoped_into_a_mount_counts_the_repository_its_scope_opens_as_an_unscoped_check_does() {
    let (library, commit) = library();
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    assert_eq!(check(&scenario, None).work.repositories, 1);
    assert_eq!(
        check(&scenario, Some("/vendor/lib/tasks"))
            .work
            .repositories,
        1
    );
}

#[test]
fn a_mount_inside_a_mounted_board_is_followed_and_reads_against_its_own_repository() {
    let (library, commit) = library();
    let middle = Scenario::new();
    let middle_commit = middle.commit(&[("deep.json", &mount(&library.path(), commit))]);
    let scenario = board();
    scenario.write("m.json", Item::File(&mount(&middle.path(), middle_commit)));
    let entries = mounted(&check(&scenario, None));
    assert!(entries.contains(&("/m/deep".to_owned(), Status::Ok)));
    assert!(entries.contains(&("/m/deep/tasks/ok".to_owned(), Status::Ok)));
}

#[test]
fn a_mount_that_cannot_be_read_degrades_only_its_pointer() {
    let (library, commit) = library();
    let scenario = board();
    scenario
        .write("good.json", Item::File(&mount(&library.path(), commit)))
        .write(
            "gone.json",
            Item::File(&mount("/no/such/repository", commit)),
        );
    let report = check(&scenario, None);
    let status = |path: &str| -> Status {
        report
            .entries
            .iter()
            .find(|entry: &&Checked| entry.source == Source::WorkingTree && entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(
        status("gone.json"),
        Status::Degraded(Code::MountRepoMissing)
    );
    assert_eq!(status("good.json"), Status::Ok);
    assert_eq!(mounted(&report).len(), 5);
}

#[test]
fn a_mounted_commit_is_listed_once_and_not_again_when_warm() {
    let (library, commit) = library();
    let scenario = board();
    let to_library = mount(&library.path(), commit);
    scenario
        .write("a.json", Item::File(&to_library))
        .write("b.json", Item::File(&to_library));
    let once = board();
    once.write("a.json", Item::File(&to_library));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let cold = board.check(None, Some(cache.path())).unwrap();
    // Eight trees in the library; the root board has no commit.
    assert_eq!(cold.work.trees, 8);
    assert_eq!(open(&once).check(None, None).unwrap().work.trees, 8);
    let warm = board.check(None, Some(cache.path())).unwrap();
    assert_eq!(
        (warm.work.trees, warm.work.parsed, warm.work.validated),
        (0, 0, 0)
    );
    assert_eq!(warm.entries, cold.entries);
}

#[test]
fn a_path_too_long_under_its_mount_point_is_malformed_and_its_shorter_twin_is_ok() {
    // Nineteen segments of 204 bytes and a last one of `last` bytes: 4091
    // bytes as a logical path in the library when `last` is 195, which fits,
    // and 4102 under /vendor/lib, which does not. One of 180 fits both.
    let path = |last: usize| {
        let mut segments = vec!["a".repeat(204); 19];
        segments.push("b".repeat(last));
        format!("{}.json", segments.join("/"))
    };
    let (long, short) = (path(195), path(180));
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        (&long, r#"{"$type": "lr/project/v1"}"#),
        (&short, r#"{"$type": "lr/project/v1"}"#),
    ]);
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    let report = check(&scenario, None);
    let status = |path: &str| -> Status {
        report
            .entries
            .iter()
            .find(|entry: &&Checked| entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(status(&long), Status::Degraded(Code::PathMalformed));
    assert_eq!(status(&short), Status::Ok);
}

#[test]
fn a_mount_whose_mount_point_is_too_long_under_its_own_mount_is_malformed_and_not_walked_and_its_shorter_twin_is()
 {
    // As above: the pointer's logical path is 4091 bytes in the library
    // when `last` is 195, and 4102 under /vendor/lib. With 170 it is 4077
    // there, and the inner board's /projects/p, 11 bytes, still fits.
    let path = |last: usize| {
        let mut segments = vec!["a".repeat(204); 19];
        segments.push("b".repeat(last));
        format!("{}.json", segments.join("/"))
    };
    let inner = Scenario::new();
    let inner_commit = inner.commit(&[
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ]);
    let pointer = mount(&inner.path(), inner_commit);
    for (last, walked) in [(195, false), (170, true)] {
        let library = Scenario::new();
        let commit = library.commit(&[(&path(last), &pointer)]);
        let scenario = board();
        scenario.write(
            "vendor/lib.json",
            Item::File(&mount(&library.path(), commit)),
        );
        let report = check(&scenario, None);
        let status = |source: &Source, path: &str| {
            let found = report.entries.iter();
            let mut found = found.filter(|entry| &entry.source == source && entry.path == path);
            found.next().map(|entry| entry.status)
        };
        let expected = if walked {
            Status::Ok
        } else {
            Status::Degraded(Code::PathMalformed)
        };
        let library = Source::Commit(commit);
        assert_eq!(status(&library, &path(last)), Some(expected), "{last}");
        let inner = Source::Commit(inner_commit);
        let project = walked.then_some(Status::Ok);
        assert_eq!(status(&inner, "projects/p.json"), project, "{last}");
    }
}

#[test]
fn a_check_opens_a_repository_mounted_at_two_commits_once_and_two_repositories_twice() {
    let (one, first) = library();
    let second = one.commit(&[("tasks/later.json", &task(""))]);
    let scenario = board();
    scenario
        .write("a.json", Item::File(&mount(&one.path(), first)))
        .write("b.json", Item::File(&mount(&one.path(), second)));
    let board = open(&scenario);
    let report = board.check(None, None).unwrap();
    let reached: Vec<String> = mounted(&report).into_iter().map(|(at, _)| at).collect();
    assert!(reached.contains(&"/a/tasks/ok".to_owned()), "{reached:?}");
    assert!(
        reached.contains(&"/b/tasks/later".to_owned()),
        "{reached:?}"
    );
    assert_eq!(board.repositories().opened(), 1);
    let (other, commit) = library();
    scenario.write("c.json", Item::File(&mount(&other.path(), commit)));
    let board = open(&scenario);
    board.check(None, None).unwrap();
    assert_eq!(board.repositories().opened(), 2);
}

#[test]
fn getting_and_listing_through_a_mount_opens_the_mounted_repository_once() {
    let (one, commit) = library();
    let scenario = board();
    scenario.write("a.json", Item::File(&mount(&one.path(), commit)));
    let board = open(&scenario);
    assert_eq!(board.repositories().opened(), 0);
    for _ in 0..2 {
        board.get(&"/a/tasks/ok".parse().unwrap()).unwrap();
        board.ls(&"/a/tasks".parse().unwrap(), None).unwrap();
    }
    assert_eq!(board.repositories().opened(), 1);
}

/// A link type whose `task` field expects a task.
const LINK_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "task": {"type": "string", "format": "board-ref", "x-board-type": "lr/task/v1"}
  }
}"#;

#[test]
fn a_reference_into_a_mount_is_typed_by_the_mounted_record_and_not_the_root_entry_at_its_path() {
    // The root and the library each hold `projects/p.json`, of different
    // types. A reference into the library must read the library's.
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ]);
    let scenario = board();
    scenario
        .write(
            ".board/types/lr/link/v1.schema.json",
            Item::File(LINK_SCHEMA),
        )
        .write("projects/p.json", Item::File(&task("")))
        .write(
            "vendor/lib.json",
            Item::File(&mount(&library.path(), commit)),
        )
        .write(
            "ok.json",
            Item::File(&task(r#", "project": "/vendor/lib/projects/p""#)),
        )
        .write(
            "wrong.json",
            Item::File(r#"{"$type": "lr/link/v1", "task": "/vendor/lib/projects/p"}"#),
        );
    let report = check(&scenario, None);
    let status = |path: &str| -> Status {
        report
            .entries
            .iter()
            .find(|entry: &&Checked| entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(status("ok.json"), Status::Ok);
    assert_eq!(status("wrong.json"), Status::Degraded(Code::RefWrongType));
}

#[test]
fn a_board_places_a_directory_of_a_repository_it_mounts_and_not_one_of_another() {
    let (library, commit) = library();
    let other = Scenario::new();
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), commit)),
    );
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    for _cold_then_warm in 0..2 {
        let places = board.places(library.workdir(), Some(cache.path())).unwrap();
        let prefixes: Vec<String> = places
            .iter()
            .map(|place| place.prefix().unwrap().into())
            .collect();
        assert_eq!(prefixes, ["/vendor/lib"]);
        let elsewhere = board.places(other.workdir(), Some(cache.path())).unwrap();
        assert_eq!(elsewhere, []);
    }
}

/// How many trees the mounted library decoded while a board checked itself,
/// when each of `notes` notes links through the mount twice: once to a
/// symlink whose long text passes through `notes/deep` 400 times, and once
/// to a file.
fn decoded_for(notes: usize) -> usize {
    let library = Scenario::new();
    let text = format!("{}a.md", "../deep/".repeat(400));
    let commit = library.commit_items(&[
        (b"notes/deep/a.md", Item::File("A.\n")),
        (b"notes/deep/l.md", Item::Symlink(&text)),
        (b"notes/other/b.md", Item::File("B.\n")),
    ]);
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
    for note in 0..notes {
        let text = "[a](/lib/notes/deep/l.md) [b](/lib/notes/other/b.md)\n";
        scenario.write(&format!("notes/{note}.md"), Item::File(text));
    }
    let board = open(&scenario);
    let report = board.check(None, None).unwrap();
    let statuses: Vec<_> = report.entries.iter().map(|entry| entry.status).collect();
    assert!(
        statuses.iter().all(|status| *status == Status::Ok),
        "{statuses:?}"
    );
    let mounted = board_model::Board {
        repository: library.workdir().to_path_buf(),
        source: Source::Commit(commit),
    };
    board
        .repositories()
        .repository(&mounted)
        .unwrap()
        .trees_decoded()
}

#[test]
fn links_through_a_mount_decode_each_tree_on_their_paths_once_however_many_and_however_long_a_symlink_text_on_them()
 {
    // Listing the mount decodes its four directories. Following the links
    // decodes them once more in all, however many notes link: the root,
    // `notes`, `deep` and `other`. The symlink's 400 passes through `deep`
    // decode nothing more.
    assert_eq!(decoded_for(0), 4);
    assert_eq!(decoded_for(1), 4 + 4);
    assert_eq!(decoded_for(20), 4 + 4);
}

#[test]
fn symlinks_in_a_mount_whose_texts_are_long_are_reported_by_their_length_and_a_short_twin_whole() {
    let long = "a/".repeat(10_000);
    let names: Vec<String> = (0..50).map(|n| format!("notes/l{n}.md")).collect();
    let mut items: Vec<(&[u8], Item<'_>)> = names
        .iter()
        .map(|name| (name.as_bytes(), Item::Symlink(&long)))
        .collect();
    items.push((b"notes/short.md", Item::Symlink("gone.md")));
    let library = Scenario::new();
    let commit = library.commit_items(&items);
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
    let report = check(&scenario, None);
    let shown = "… (over 4096 bytes)".to_owned();
    let mut links = 0;
    for entry in &report.entries {
        let Some(detail) = &entry.detail else {
            continue;
        };
        links += 1;
        assert_eq!(entry.status, Status::Degraded(Code::SymlinkDangling));
        let expected = if entry.path == "notes/short.md" {
            "gone.md"
        } else {
            &shown
        };
        assert_eq!(detail.target.as_deref(), Some(expected), "{}", entry.path);
        let hint = entry.hint().unwrap();
        assert!(
            detail.reason.len() + hint.len() < 1_000,
            "{detail:?} {hint}"
        );
    }
    assert_eq!(links, 51);
    // Fifty texts of 20,000 bytes would make a report of over a megabyte.
    let size = format!("{report:?}").len();
    assert!(size < 100_000, "{size} bytes");
}

#[test]
fn a_binding_in_a_mounted_board_fits_the_view_in_its_own_board_and_one_whose_view_is_missing_there_dangles()
 {
    let view = r#"{"$type": "ui/view/v1", "slots": [{"name": "s", "kind": "markdown"}]}"#;
    let binding = r#"{"$type": "ui/binding/v1", "view": "/views/v",
        "slots": {"s": {"data": {"markdown": "x"}}}}"#;
    // The root's view at the same path names another slot, so the binding
    // read against the root would not fit.
    let other = view.replace(r#""s""#, r#""t""#);
    let cases = [
        (
            vec![("views/v.json", view), ("bindings/b.json", binding)],
            None,
        ),
        (
            vec![("bindings/b.json", binding)],
            Some((Code::RefDangling, "/view")),
        ),
    ];
    for (files, expected) in cases {
        let library = Scenario::new();
        let commit = library.commit(&files);
        let scenario = board();
        scenario.write("views/v.json", Item::File(&other)).write(
            "vendor/lib.json",
            Item::File(&mount(&library.path(), commit)),
        );
        let report = check(&scenario, None);
        let entry = (report.entries.iter())
            .find(|entry| entry.logical.as_ref().unwrap().as_str() == "/vendor/lib/bindings/b")
            .unwrap();
        let found = match entry.status {
            Status::Ok => None,
            Status::Degraded(code) => {
                let detail = entry.detail.as_ref().unwrap();
                Some((code, detail.pointer.as_deref().unwrap()))
            }
        };
        assert_eq!(found, expected);
    }
}

#[test]
fn a_plain_record_that_becomes_a_link_between_warm_runs_is_followed_in_the_working_tree_and_in_a_mount()
 {
    let library = Scenario::new();
    let project = r#"{"$type": "lr/project/v1"}"#;
    let to_q = r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#;
    let files = |p: &'static str| {
        [
            (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
            ("projects/p.json", p),
            ("projects/q.json", project),
        ]
    };
    let (plain, linked) = (
        library.commit(&files(project)),
        library.commit(&files(to_q)),
    );
    let scenario = board();
    let refer = |target: &str| task(&format!(r#", "project": "{target}""#));
    scenario
        .write("projects/q.json", Item::File(project))
        .write("vendor.json", Item::File(&mount(&library.path(), plain)))
        .write("tasks/a.json", Item::File(&refer("/projects/p")))
        .write("tasks/b.json", Item::File(&refer("/vendor/projects/p")));
    let cache = tempfile::tempdir().unwrap();
    let root = open(&scenario).repository().path().to_path_buf();
    let warm = || board_index::Index::open(Some(cache.path()), &root).unwrap();
    let led = |scenario: &Scenario| {
        super::agrees(scenario, &warm).unwrap();
        super::led(&super::filled(&open(scenario), warm()).references)
    };
    let reached = |task: &str, path: &str, pointers| {
        (
            format!("tasks/{task}.json"),
            Some((path.to_owned(), pointers)),
        )
    };
    let before = [
        reached("a", "projects/p.json", 0),
        reached("b", "projects/p.json", 1),
    ];
    assert_eq!(led(&scenario), before);
    scenario
        .write("projects/p.json", Item::File(to_q))
        .write("vendor.json", Item::File(&mount(&library.path(), linked)));
    let after = [
        reached("a", "projects/q.json", 1),
        reached("b", "projects/q.json", 2),
    ];
    assert_eq!(led(&scenario), after);
}

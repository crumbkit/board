//! A put through one mount of a board writes in the mounted repository,
//! then moves the mount's pin to the commit it made.

use std::time::Duration;

use board_model::{Code, Fields, ObjectId, Source};
use board_scenario::seed::mount_of;
use board_scenario::{Item, Scenario};
use board_tree::LOCK_WAIT;

use super::{board, canonical, file, path, put};
use crate::Board;
use crate::put::{Expect, PutError, PutReport};

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, open, task};

const IDENTITY: &str = "[user]\n\tname = Board Test\n\temail = board@example.com\n";

/// A board in a repository of its own, committed and tracked, with an
/// identity, that commits its puts, and the commit HEAD names.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ];
    for (at, text) in files {
        library.write(at, Item::File(text));
    }
    let commit = library.commit(&files);
    library
        .track(commit)
        .write(".git/config", Item::Appended(IDENTITY));
    (library, commit)
}

/// Commits `files` in `library` onto HEAD, in its working tree and index.
fn commit_in(library: &Scenario, files: &[(&str, &str)]) -> ObjectId {
    let mut all: Vec<(String, String)> = library
        .head()
        .unwrap()
        .files
        .into_iter()
        .map(|(at, bytes)| (at, String::from_utf8(bytes).unwrap()))
        .collect();
    for (at, text) in files {
        all.retain(|(held, _)| held != at);
        all.push(((*at).to_owned(), (*text).to_owned()));
        library.write(at, Item::File(text));
    }
    let borrowed: Vec<(&str, &str)> = all.iter().map(|(a, t)| (a.as_str(), t.as_str())).collect();
    let commit = library.commit(&borrowed);
    library.track(commit);
    commit
}

/// The test board, mounting `library` at `/vendor` pinned to `commit`.
fn mounting(library: &Scenario, commit: ObjectId) -> Scenario {
    let scenario = board();
    pin(&scenario, library, commit);
    scenario
}

fn pin(scenario: &Scenario, library: &Scenario, commit: ObjectId) {
    scenario.write(
        "vendor.json",
        Item::File(&mount_of(&library.path(), commit)),
    );
}

/// The commit the mount at `/vendor` is pinned to.
fn pinned(scenario: &Scenario) -> String {
    let pointer: serde_json::Value =
        serde_json::from_slice(&file(scenario, "vendor.json").unwrap()).unwrap();
    pointer["commit"].as_str().unwrap().to_owned()
}

/// Asserts that git would find nothing to commit in `library`: its index
/// holds exactly HEAD's files, and each working-tree file holds HEAD's bytes.
fn assert_clean(library: &Scenario) {
    let head = library.head().unwrap();
    let index = library.index();
    assert_eq!(
        index.keys().collect::<Vec<_>>(),
        head.files.keys().collect::<Vec<_>>()
    );
    for (at, bytes) in &head.files {
        assert_eq!(index[at], library.blob(std::str::from_utf8(bytes).unwrap()));
        assert_eq!(
            &std::fs::read(library.workdir().join(at)).unwrap(),
            bytes,
            "{at}"
        );
    }
}

#[test]
fn a_put_through_a_mount_of_a_board_commits_there_and_moves_the_pin_and_one_into_a_plain_repository_is_refused()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let record = task(r#", "project": "/projects/p""#);
    let report = put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap();
    let head = library.head().unwrap();
    assert_eq!(head.message, "board put /tasks/a");
    assert_eq!(head.parents, [commit]);
    assert_eq!(head.files["tasks/a.json"], canonical(&record));
    assert_eq!(pinned(&scenario), head.id.to_string());
    let written = &report.writes[0];
    assert_eq!(written.display, path("/vendor/tasks/a"));
    assert_eq!(written.location.board.source, Source::Commit(head.id));
    assert_eq!(written.location.path, "tasks/a.json");
    assert_eq!((written.previous, &written.degraded), (None, &None));
    assert_eq!(report.commit, None);
    assert_clean(&library);
    assert!(!open(&scenario).check(None, None).unwrap().degraded());

    library.write(".board/board.json", Item::Deleted);
    let refused = open(&scenario)
        .put(&path("/vendor/tasks/b"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
    assert_eq!(
        refused.to_string(),
        "/vendor/tasks/b cannot be written: it is in the board mounted at /vendor, \
         whose repository has no working tree with a .board/board.json"
    );
    assert_eq!(library.head().unwrap().id, head.id);
}

#[test]
fn a_binding_put_through_a_mount_fits_the_view_in_the_mounted_board_and_one_whose_view_is_missing_there_is_degraded()
 {
    let (library, _) = library();
    let view = r#"{"$type": "ui/view/v1", "slots": [{"name": "s", "kind": "markdown"}]}"#;
    let commit = commit_in(&library, &[("views/v.json", view)]);
    let scenario = mounting(&library, commit);
    let binding = |view: &str| {
        format!(
            r#"{{"$type": "ui/binding/v1", "view": "{view}", "slots": {{"s": {{"data": {{"markdown": "x"}}}}}}}}"#
        )
    };
    let degraded = |at: &str, view: &str| {
        let report = put(&scenario, at, Expect::Absent, &binding(view)).unwrap();
        (report.writes[0].degraded.clone()).map(|(code, detail)| (code, detail.pointer))
    };
    assert_eq!(degraded("/vendor/bindings/a", "/views/v"), None);
    assert_eq!(
        degraded("/vendor/bindings/b", "/views/w"),
        Some((Code::RefDangling, Some("/view".to_owned())))
    );
}

#[test]
fn a_root_that_commits_its_puts_commits_the_moved_pin() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(".git/config", Item::Appended(IDENTITY));
    let report = put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap();
    let root = scenario.head().unwrap();
    assert_eq!(report.commit, Some(root.id));
    assert_eq!(root.message, "board put /vendor/tasks/a");
    assert_eq!(root.files.keys().collect::<Vec<_>>(), ["vendor.json"]);
    assert_eq!(pinned(&scenario), library.head().unwrap().id.to_string());
}

#[test]
fn a_mounted_board_that_does_not_commit_its_puts_is_refused_and_one_that_does_is_admitted() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    library.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let refused = open(&scenario)
        .put(&path("/vendor/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert!(
        refused
            .to_string()
            .ends_with("whose board sets commit_on_put to false")
    );
    assert!(!library.workdir().join("tasks/a.json").exists());
    library.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    assert!(put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_mounted_head_off_the_local_branch_the_mount_follows_is_refused_and_one_on_it_is_admitted() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let follow = |follow: &str| {
        scenario.write(
            "vendor.json",
            Item::File(&mount_of(&library.path(), commit).replace(r#""main""#, follow)),
        );
    };
    library
        .write(".git/refs/heads/other", Item::File(&format!("{commit}\n")))
        .write(
            ".git/refs/remotes/origin/main",
            Item::File(&format!("{commit}\n")),
        );
    let cases: [(&str, &str, Code, &str); 4] = [
        (
            r#""main""#,
            &format!("{commit}\n"),
            Code::PutThroughMount,
            "whose HEAD is detached, not on refs/heads/main",
        ),
        (
            r#""main""#,
            "ref: refs/heads/other\n",
            Code::PutThroughMount,
            "whose HEAD is on refs/heads/other, not on refs/heads/main",
        ),
        (
            r#""origin/main""#,
            "ref: refs/heads/main\n",
            Code::PutThroughMount,
            "whose follow origin/main is not a local branch",
        ),
        (
            r#""gone""#,
            "ref: refs/heads/main\n",
            Code::MountFollowMissing,
            "the mount at /vendor cannot be updated: board::mount::follow_missing",
        ),
    ];
    for (followed, head, code, why) in cases {
        follow(followed);
        library.write(".git/HEAD", Item::File(head));
        let refused = open(&scenario)
            .put(&path("/vendor/tasks/a"), Expect::Any, task("").as_bytes())
            .unwrap_err();
        assert_eq!(refused.code(), code, "{why}");
        assert!(refused.to_string().ends_with(why), "{refused}");
        assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
    }
    follow(r#""main""#);
    assert!(put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_mounted_head_that_does_not_descend_from_the_pin_is_refused_and_one_that_does_is_admitted() {
    let (library, first) = library();
    let second = commit_in(&library, &[("notes/n.json", "{}")]);
    let scenario = mounting(&library, second);
    library.write(".git/refs/heads/main", Item::File(&format!("{first}\n")));
    let refused = open(&scenario)
        .put(&path("/vendor/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::MountNotAncestor);
    assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
    assert!(
        refused.to_string().ends_with(&format!(
            "whose HEAD {first} does not descend from the mount's commit {second}"
        )),
        "{refused}"
    );
    // HEAD is behind the pin; a HEAD ahead of it is admitted.
    library.write(".git/refs/heads/main", Item::File(&format!("{second}\n")));
    pin(&scenario, &library, first);
    put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap();
    assert_eq!(library.head().unwrap().parents, [second]);
}

#[test]
fn a_record_whose_schema_only_the_mounted_board_holds_is_admitted_and_one_neither_holds_is_refused()
{
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let note = r#"{"$type": "lr/note/v1"}"#;
    assert_eq!(
        put(&scenario, "/vendor/notes/n", Expect::Any, note).unwrap_err(),
        Code::RecordUnknownType
    );
    library.write(
        ".board/types/lr/note/v1.schema.json",
        Item::File(r#"{"type": "object"}"#),
    );
    assert_eq!(
        put(&scenario, "/notes/n", Expect::Any, note).unwrap_err(),
        Code::RecordUnknownType
    );
    put(&scenario, "/vendor/notes/n", Expect::Any, note).unwrap();
    assert_eq!(
        library.head().unwrap().files["notes/n.json"],
        canonical(note)
    );
}

#[test]
fn a_record_changed_in_the_mounted_repository_since_the_pin_is_refused_and_an_unchanged_one_is_admitted()
 {
    let (library, commit) = library();
    let first = commit_in(&library, &[("tasks/a.json", &task(""))]);
    let scenario = mounting(&library, first);
    let changed = task(r#", "see": "/x""#);
    // Changed in the working tree.
    library.write("tasks/a.json", Item::File(&changed));
    let refused = open(&scenario)
        .put(&path("/vendor/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::MountStale);
    assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
    // Changed at HEAD.
    let second = commit_in(&library, &[("tasks/a.json", &changed)]);
    assert_eq!(
        put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap_err(),
        Code::MountStale
    );
    // A file beside it that changed does not matter.
    pin(&scenario, &library, second);
    commit_in(&library, &[("tasks/b.json", &task(""))]);
    put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap();
    assert_ne!(commit, second);
}

#[test]
fn an_expect_is_compared_with_the_pinned_blob_that_get_shows() {
    let (library, _) = library();
    let first = commit_in(&library, &[("tasks/a.json", &task(""))]);
    let scenario = mounting(&library, first);
    let blob = library.blob(&task(""));
    assert_eq!(
        open(&scenario).get(&path("/vendor/tasks/a")).unwrap().bytes,
        task("").as_bytes()
    );
    let other = ObjectId::Sha1([7; 20]);
    let refused = open(&scenario)
        .put(
            &path("/vendor/tasks/a"),
            Expect::Blob(other),
            task("").as_bytes(),
        )
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutStale);
    assert_eq!(refused.fields().blob, Some(blob));
    assert_eq!(refused.fields().path, Some(path("/vendor/tasks/a")));
    assert_eq!(
        put(&scenario, "/vendor/tasks/a", Expect::Absent, &task("")).unwrap_err(),
        Code::PutStale
    );
    let report = put(
        &scenario,
        "/vendor/tasks/a",
        Expect::Blob(blob),
        &task(r#", "see": "/x""#),
    )
    .unwrap();
    assert_eq!(report.writes[0].previous, Some(blob));
}

#[test]
fn a_write_through_two_mounts_is_refused_and_one_through_a_link_to_one_mount_is_admitted() {
    let (inner, inner_commit) = library();
    let (library, _) = library();
    let commit = commit_in(
        &library,
        &[("inner.json", &mount_of(&inner.path(), inner_commit))],
    );
    let scenario = mounting(&library, commit);
    let refused = open(&scenario)
        .put(
            &path("/vendor/inner/tasks/a"),
            Expect::Any,
            task("").as_bytes(),
        )
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert!(
        refused
            .to_string()
            .ends_with("and a put writes through one mount at most"),
        "{refused}"
    );
    assert_eq!(inner.head().unwrap().id, inner_commit);
    scenario.write(
        "v.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/vendor/projects"}"#),
    );
    let project = r#"{"$type": "lr/project/v1"}"#;
    let report = put(&scenario, "/v/q", Expect::Any, project).unwrap();
    assert_eq!(report.writes[0].display, path("/vendor/projects/q"));
    assert!(
        library
            .head()
            .unwrap()
            .files
            .contains_key("projects/q.json")
    );
}

/// A batch line at `path` holding `field`, which is `record` or `text`.
fn batch_line(path: &str, field: &str, value: &str) -> String {
    format!(r#"{{"path": "{path}", "{field}": {value}}}"#) + "\n"
}

/// A batch through `/vendor`: a schema, a record of its type that refers to
/// a file, and the file.
fn shared() -> String {
    batch_line(
        "/vendor/notes/a",
        "record",
        r#"{"$type": "lr/note/v1", "see": "/notes/a.md"}"#,
    ) + &batch_line("/vendor/notes/a.md", "text", "\"# A\\n\"")
        + &batch_line(
            "/vendor/.board/types/lr/note/v1.schema",
            "record",
            r#"{"type": "object"}"#,
        )
}

fn put_batch(scenario: &Scenario, input: &str) -> Result<PutReport, PutError> {
    open(scenario).put_batch(&mut input.as_bytes())
}

#[test]
fn a_batch_of_a_schema_records_and_files_through_a_mount_commits_once_there_and_moves_the_pin() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let report = put_batch(&scenario, &shared()).unwrap();
    let head = library.head().unwrap();
    assert_eq!(head.message, "board put --batch (2 records, 1 files)");
    assert_eq!(head.parents, [commit]);
    assert_eq!(
        head.files[".board/types/lr/note/v1.schema.json"],
        canonical(r#"{"type": "object"}"#)
    );
    assert_eq!(head.files["notes/a.md"], b"# A\n");
    assert!(head.files.contains_key("notes/a.json"));
    assert_eq!(pinned(&scenario), head.id.to_string());
    let displays: Vec<String> = report
        .writes
        .iter()
        .map(|w| w.display.to_string())
        .collect();
    assert_eq!(
        displays,
        [
            "/vendor/notes/a",
            "/vendor/notes/a.md",
            "/vendor/.board/types/lr/note/v1.schema"
        ]
    );
    assert!(
        report
            .writes
            .iter()
            .all(|w| w.location.board.source == Source::Commit(head.id))
    );
    assert_eq!(report.commit, None);
    assert_clean(&library);
    assert!(!open(&scenario).check(None, None).unwrap().degraded());
    // Again, with everything in place, nothing is written and the pin stays.
    put_batch(&scenario, &shared()).unwrap();
    assert_eq!(library.head().unwrap().id, head.id);
    assert_eq!(pinned(&scenario), head.id.to_string());
}

#[test]
fn after_a_kill_left_a_batch_uncommitted_in_the_mounted_board_the_same_batch_commits_its_schema_too()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let schema = canonical(r#"{"type": "object"}"#);
    let note = canonical(r#"{"$type": "lr/note/v1", "see": "/notes/a.md"}"#);
    // The kill came after the files were written and before the commit.
    let written: [(&str, &[u8]); 3] = [
        (".board/types/lr/note/v1.schema.json", &schema),
        ("notes/a.json", &note),
        ("notes/a.md", b"# A\n"),
    ];
    for (at, bytes) in written {
        library.write(at, Item::File(std::str::from_utf8(bytes).unwrap()));
    }
    put_batch(&scenario, &shared()).unwrap();
    let head = library.head().unwrap();
    assert_eq!(head.parents, [commit]);
    assert_eq!(head.files[".board/types/lr/note/v1.schema.json"], schema);
    assert_eq!(head.files["notes/a.json"], note);
    assert_eq!(pinned(&scenario), head.id.to_string());
    assert_clean(&library);
}

#[test]
fn a_batch_through_a_mount_off_its_branch_or_not_committing_is_refused_writing_nothing_and_its_twin_is_admitted()
 {
    let (library, first) = library();
    let project = r#"{"$type": "lr/project/v1"}"#;
    let commit = commit_in(&library, &[("projects/q.json", project)]);
    let scenario = mounting(&library, commit);
    let at_commit = format!("{commit}\n");
    library
        .write(".git/refs/heads/other", Item::File(&at_commit))
        .write(".git/refs/remotes/origin/main", Item::File(&at_commit));
    let mount = mount_of(&library.path(), commit);
    let cases = [
        (
            &library,
            ".git/HEAD",
            "ref: refs/heads/other\n".to_owned(),
            "ref: refs/heads/main\n".to_owned(),
            Code::PutThroughMount,
            "whose HEAD is on refs/heads/other, not on refs/heads/main".to_owned(),
        ),
        (
            &library,
            ".board/board.json",
            r#"{"$type": "board/config/v1", "commit_on_put": false}"#.to_owned(),
            r#"{"$type": "board/config/v1"}"#.to_owned(),
            Code::PutThroughMount,
            "whose board sets commit_on_put to false".to_owned(),
        ),
        (
            &scenario,
            "vendor.json",
            mount.replace(r#""main""#, r#""origin/main""#),
            mount.clone(),
            Code::PutThroughMount,
            "whose follow origin/main is not a local branch".to_owned(),
        ),
        (
            &library,
            ".git/refs/heads/main",
            format!("{first}\n"),
            at_commit.clone(),
            Code::MountNotAncestor,
            format!("whose HEAD {first} does not descend from the mount's commit {commit}"),
        ),
    ];
    for (board, at, text, twin, code, why) in cases {
        board.write(at, Item::File(&text));
        let head = library.head().unwrap().id;
        let refused = put_batch(&scenario, &shared()).unwrap_err();
        assert_eq!(refused.code(), code, "{why}");
        assert_eq!(refused.fields(), Fields::at(&path("/vendor")));
        assert!(refused.to_string().ends_with(&why), "{refused}");
        assert_eq!(library.head().unwrap().id, head);
        assert!(!library.workdir().join("notes").exists());
        assert_eq!(pinned(&scenario), commit.to_string());
        board.write(at, Item::File(&twin));
    }
    put_batch(&scenario, &shared()).unwrap();
    assert_eq!(library.head().unwrap().parents, [commit]);
}

#[test]
fn a_batch_the_mounted_board_refuses_names_the_callers_paths_and_leaves_both_boards_and_its_twin_is_admitted()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let unknown = batch_line("/vendor/tasks/x", "record", r#"{"$type": "lr/missing/v1"}"#);
    let expect =
        r#"{"path": "/vendor/.board/types/lr/other/v1.schema", "expect": "none", "record": {}}"#;
    let input = shared() + &unknown + expect + "\n";
    let PutError::Batch { refusals, .. } = put_batch(&scenario, &input).unwrap_err() else {
        panic!("not a batch refusal");
    };
    let lines: Vec<_> = refusals.iter().map(|r| (r.line, r.code)).collect();
    assert_eq!(lines, [(4, Code::RecordUnknownType), (5, Code::PutInvalid)]);
    assert_eq!(refusals[0].fields.path, Some(path("/vendor/tasks/x")));
    assert!(
        refusals[1]
            .message
            .starts_with("/vendor/.board/types/lr/other/v1.schema is a schema"),
        "{}",
        refusals[1].message
    );
    assert_eq!(library.head().unwrap().id, commit);
    assert_clean(&library);
    assert!(!library.workdir().join("notes").exists());
    assert_eq!(pinned(&scenario), commit.to_string());
    assert!(scenario.head().is_none());
    put_batch(&scenario, &shared()).unwrap();
    assert_eq!(library.head().unwrap().parents, [commit]);
}

/// A batch line that writes the task schema `library()` holds.
fn task_schema_line() -> String {
    let schema: serde_json::Value = serde_json::from_str(TASK_SCHEMA).unwrap();
    batch_line(
        "/vendor/.board/types/lr/task/v1.schema",
        "record",
        &schema.to_string(),
    )
}

/// The refusals of a batch, as (line, code, path field, message).
fn refusals(result: Result<PutReport, PutError>) -> Vec<(usize, Code, Option<String>, String)> {
    let PutError::Batch { refusals, .. } = result.unwrap_err() else {
        panic!("not a batch refusal");
    };
    let refusals = refusals.into_iter();
    refusals
        .map(|r| {
            (
                r.line,
                r.code,
                r.fields.path.map(|p| p.to_string()),
                r.message,
            )
        })
        .collect()
}

#[test]
fn two_batch_lines_through_a_mount_writing_one_file_name_the_callers_paths_and_two_files_are_admitted()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let a = batch_line("/vendor/tasks/a", "record", &task(""));
    let upper = batch_line("/vendor/tasks/A", "record", &task(""));
    let b = batch_line("/vendor/tasks/b", "record", &task(""));
    for (second, code, shown, message) in [
        (
            &a,
            Code::PutInvalid,
            "/vendor/tasks/a",
            "it writes /vendor/tasks/a, the same file as line 1's /vendor/tasks/a",
        ),
        (
            &upper,
            Code::PathCaseCollision,
            "/vendor/tasks/A",
            "it writes /vendor/tasks/A, differing only in case from line 1's /vendor/tasks/a",
        ),
    ] {
        assert_eq!(
            refusals(put_batch(&scenario, &(a.clone() + second))),
            [(2, code, Some(shown.to_owned()), message.to_owned())]
        );
        assert_eq!(library.head().unwrap().id, commit);
    }
    put_batch(&scenario, &(a + &b)).unwrap();
    assert_eq!(library.head().unwrap().parents, [commit]);
}

#[test]
fn a_batch_line_or_put_that_differs_in_case_from_a_mounted_file_names_the_callers_path_and_its_twin_is_admitted()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let project = r#"{"$type": "lr/project/v1"}"#;
    let upper = batch_line("/vendor/projects/P", "record", project);
    let [(line, code, path, _)] = &refusals(put_batch(&scenario, &upper))[..] else {
        panic!("not one refusal");
    };
    assert_eq!((*line, *code), (1, Code::PathCaseCollision));
    assert_eq!(path.as_deref(), Some("/vendor/projects/P"));
    let at = self::path("/vendor/projects/P");
    let single = open(&scenario).put(&at, Expect::Any, project.as_bytes());
    let single = single.unwrap_err();
    assert_eq!(
        (single.code(), single.fields().path),
        (Code::PathCaseCollision, Some(at))
    );
    assert_eq!(library.head().unwrap().id, commit);
    put_batch(
        &scenario,
        &batch_line("/vendor/projects/p", "record", project),
    )
    .unwrap();
    put(&scenario, "/vendor/projects/p", Expect::Any, project).unwrap();
}

#[test]
fn a_schema_line_whose_schema_the_mounted_working_tree_deleted_is_mount_stale_and_restored_is_admitted()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let at = ".board/types/lr/task/v1.schema.json";
    library.write(at, Item::Deleted);
    let schema = task_schema_line();
    let [(line, code, _, _)] = &refusals(put_batch(&scenario, &schema))[..] else {
        panic!("not one refusal");
    };
    assert_eq!((*line, *code), (1, Code::MountStale));
    assert_eq!(library.head().unwrap().id, commit);
    library.write(at, Item::File(TASK_SCHEMA));
    put_batch(&scenario, &schema).unwrap();
}

#[test]
fn a_batch_of_a_schema_the_mounted_board_holds_moves_the_pin_to_a_head_ahead_of_it() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let project = r#"{"$type": "lr/project/v1"}"#;
    let ahead = commit_in(&library, &[("projects/q.json", project)]);
    let schema = task_schema_line();
    let report = put_batch(&scenario, &schema).unwrap();
    assert_eq!(library.head().unwrap().id, ahead);
    assert_eq!(pinned(&scenario), ahead.to_string());
    assert_eq!(report.writes.len(), 1);
    assert_eq!(report.writes[0].previous, Some(report.writes[0].blob));
}

#[test]
fn a_batch_that_writes_in_two_boards_is_refused_and_each_board_alone_is_admitted() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let (other, other_commit) = self::library();
    scenario.write(
        "other.json",
        Item::File(&mount_of(&other.path(), other_commit)),
    );
    let here = batch_line("/tasks/a", "record", &task(""));
    let vendor = batch_line("/vendor/tasks/a", "record", &task(""));
    let elsewhere = batch_line("/other/tasks/a", "record", &task(""));
    for (input, line, message) in [
        (
            here.clone() + &vendor,
            2,
            "it writes in the board mounted at /vendor, and line 1 writes in this board; a batch writes in one board",
        ),
        // A schema line is checked first, and line 1 is still the one compared.
        (
            vendor.clone() + &batch_line("/.board/types/lr/note/v1.schema", "record", "{}"),
            2,
            "it writes in this board, and line 1 writes in the board mounted at /vendor; a batch writes in one board",
        ),
        (
            vendor.clone() + &elsewhere,
            2,
            "it writes in the board mounted at /other, and line 1 writes in the board mounted at /vendor; a batch writes in one board",
        ),
    ] {
        let PutError::Batch { refusals, .. } = put_batch(&scenario, &input).unwrap_err() else {
            panic!("not a batch refusal");
        };
        assert_eq!(refusals.len(), 1);
        assert_eq!(
            (refusals[0].line, refusals[0].code),
            (line, Code::PutThroughMount)
        );
        assert_eq!(refusals[0].message, message);
        assert!(file(&scenario, "tasks/a.json").is_none());
        assert_eq!(library.head().unwrap().id, commit);
    }
    put_batch(&scenario, &here).unwrap();
    put_batch(&scenario, &vendor).unwrap();
    assert!(library.head().unwrap().files.contains_key("tasks/a.json"));
}

#[test]
fn a_batch_line_changed_since_the_pin_or_stale_refuses_the_batch_and_its_twin_is_admitted() {
    let (library, _) = library();
    let first = commit_in(&library, &[("tasks/a.json", &task(""))]);
    let scenario = mounting(&library, first);
    let blob = library.blob(&task(""));
    let b = batch_line("/vendor/tasks/b", "record", &task(""));
    let stale = format!(
        r#"{{"path": "/vendor/tasks/a", "expect": "none", "record": {}}}"#,
        task("")
    ) + "\n";
    let PutError::Batch { code, refusals } =
        put_batch(&scenario, &(b.clone() + &stale)).unwrap_err()
    else {
        panic!("not a batch refusal");
    };
    assert_eq!(code, Code::PutStale);
    assert_eq!((refusals[0].line, refusals[0].code), (2, Code::PutStale));
    assert_eq!(refusals[0].fields.blob, Some(blob));
    assert_eq!(refusals[0].fields.path, Some(path("/vendor/tasks/a")));
    library.write("tasks/a.json", Item::File(&task(r#", "see": "/x""#)));
    let changed = batch_line("/vendor/tasks/a", "record", &task(""));
    let PutError::Batch { code, refusals } =
        put_batch(&scenario, &(b.clone() + &changed)).unwrap_err()
    else {
        panic!("not a batch refusal");
    };
    assert_eq!(code, Code::PutInvalid);
    assert_eq!((refusals[0].line, refusals[0].code), (2, Code::MountStale));
    assert_eq!(library.head().unwrap().id, first);
    library.write("tasks/a.json", Item::File(&task("")));
    put_batch(&scenario, &(b + &changed)).unwrap();
    assert_eq!(library.head().unwrap().parents, [first]);
}

#[test]
fn a_failed_mounted_put_leaves_the_pin_and_a_put_that_succeeds_moves_it() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    library.write(".git/index.lock", Item::File(""));
    assert_eq!(
        put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap_err(),
        Code::PutLocked
    );
    assert_eq!(pinned(&scenario), commit.to_string());
    assert_eq!(library.head().unwrap().id, commit);
    library.write(".git/index.lock", Item::Deleted);
    put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap();
    assert_eq!(pinned(&scenario), library.head().unwrap().id.to_string());
}

#[test]
fn a_mounted_repository_whose_nearest_board_is_an_enclosing_one_is_refused_and_one_with_its_own_is_admitted()
 {
    let scenario = board();
    let library = Scenario::within(&scenario);
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
    ];
    for (at, text) in files {
        library.write(at, Item::File(text));
    }
    let commit = library.commit(&files);
    library
        .track(commit)
        .write(".git/config", Item::Appended(IDENTITY));
    pin(&scenario, &library, commit);
    // Without its own board.json, the nearest board above the mounted
    // repository is the root's, which is not the repository the mount names.
    library.write(".board/board.json", Item::Deleted);
    let refused = open(&scenario)
        .put(&path("/vendor/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert!(
        refused
            .to_string()
            .ends_with("whose repository has no working tree with a .board/board.json"),
        "{refused}"
    );
    assert!(!scenario.workdir().join("tasks/a.json").exists());
    library.write(".board/board.json", Item::File(files[0].1));
    assert!(put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_home_mount_in_the_mounted_board_resolves_against_the_roots_home_and_without_one_the_reference_dangles()
 {
    let (library, _) = library();
    let (other, other_commit) = self::library();
    let home = other.workdir().parent().unwrap().to_path_buf();
    let name = other.workdir().file_name().unwrap().to_str().unwrap();
    let commit = commit_in(
        &library,
        &[("other.json", &mount_of(&format!("~/{name}"), other_commit))],
    );
    let scenario = mounting(&library, commit);
    let record = task(r#", "project": "/other/projects/p""#);
    let at = path("/vendor/tasks/a");
    let degraded = |home| {
        Board::open(scenario.workdir(), home, "")
            .unwrap()
            .put(&at, Expect::Any, record.as_bytes())
            .unwrap()
            .writes[0]
            .degraded
            .clone()
    };
    assert_eq!(degraded(None).unwrap().0, Code::RefDangling);
    assert_eq!(degraded(Some(home)), None);
}

#[test]
fn a_mount_of_the_boards_own_repository_is_refused_without_waiting_and_another_is_admitted() {
    let scenario = board();
    let own = scenario.commit(&[(".board/board.json", r#"{"$type": "board/config/v1"}"#)]);
    scenario.write("self.json", Item::File(&mount_of(&scenario.path(), own)));
    let refused = open(&scenario)
        .put(&path("/self/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert!(
        refused
            .to_string()
            .ends_with("which is this board's own repository")
    );
    let (library, commit) = library();
    pin(&scenario, &library, commit);
    assert!(put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_put_through_a_mount_whose_board_is_locked_is_refused_and_one_after_it_is_released_is_admitted()
{
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let mounted = open(&library);
    let held = mounted.repository().lock(LOCK_WAIT).unwrap();
    let put_now = || {
        open(&scenario).put_waiting(
            &path("/vendor/tasks/a"),
            Expect::Any,
            task("").as_bytes(),
            Duration::ZERO,
        )
    };
    assert_eq!(put_now().unwrap_err().code(), Code::PutLocked);
    assert_eq!(pinned(&scenario), commit.to_string());
    drop(held);
    assert!(put_now().is_ok());
}

#[test]
fn after_a_kill_left_the_record_committed_the_same_put_moves_the_pin_without_another_commit() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let record = task("");
    let text = String::from_utf8(canonical(&record)).unwrap();
    let killed = commit_in(&library, &[("tasks/a.json", &text)]);
    let report = put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap();
    assert_eq!(library.head().unwrap().id, killed);
    assert_eq!(pinned(&scenario), killed.to_string());
    assert_eq!(
        report.writes[0].location.board.source,
        Source::Commit(killed)
    );
    // Again, with nothing left to do, nothing is written.
    let again = put(&scenario, "/vendor/tasks/a", Expect::Any, &record).unwrap();
    assert_eq!(library.head().unwrap().id, killed);
    assert_eq!(again.writes[0].previous, Some(again.writes[0].blob));
    // A different record at HEAD is not a finished put.
    commit_in(&library, &[("tasks/a.json", &task(r#", "see": "/x""#))]);
    assert_eq!(
        put(&scenario, "/vendor/tasks/a", Expect::Any, &record).unwrap_err(),
        Code::MountStale
    );
}

#[test]
fn a_record_written_through_a_mount_is_degraded_as_the_root_sees_the_moved_pin() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    // In the mounted working tree, but not in any commit.
    library.write(
        "projects/q.json",
        Item::File(r#"{"$type": "lr/project/v1"}"#),
    );
    let report: PutReport = put(
        &scenario,
        "/vendor/tasks/a",
        Expect::Any,
        &task(r#", "project": "/projects/q""#),
    )
    .unwrap();
    assert_eq!(
        report.writes[0].degraded.as_ref().unwrap().0,
        Code::RefDangling
    );
    let report = put(
        &scenario,
        "/vendor/tasks/b",
        Expect::Any,
        &task(r#", "project": "/projects/p""#),
    )
    .unwrap();
    assert_eq!(report.writes[0].degraded, None);
}

#[test]
fn check_and_query_answer_from_a_warm_index_as_from_a_cold_one_after_a_write_through() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let cache = tempfile::tempdir().unwrap();
    let sql = "SELECT source, path, logical, blob FROM records ORDER BY source, path";
    let board = open(&scenario);
    board.check(None, Some(cache.path())).unwrap();
    board.query(sql, 100, None, Some(cache.path())).unwrap();
    put(&scenario, "/vendor/tasks/a", Expect::Any, &task("")).unwrap();
    let board = open(&scenario);
    let warm = board.check(None, Some(cache.path())).unwrap();
    let cold = board.check(None, None).unwrap();
    assert_eq!(warm.entries, cold.entries);
    assert!(
        warm.entries
            .iter()
            .any(|entry| entry.logical.as_ref() == Some(&path("/vendor/tasks/a")))
    );
    let rows = |cache| board.query(sql, 100, None, cache).unwrap().rows;
    assert_eq!(rows(Some(cache.path())), rows(None));
}

#[test]
fn an_uncommitted_file_that_already_holds_the_record_is_committed_and_one_holding_another_is_refused()
 {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    let record = task("");
    library.write("tasks/a.json", Item::File(&task(r#", "see": "/x""#)));
    assert_eq!(
        put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap_err(),
        Code::MountStale
    );
    let text = String::from_utf8(canonical(&record)).unwrap();
    library.write("tasks/a.json", Item::File(&text));
    put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap();
    let head = library.head().unwrap();
    assert_eq!(head.parents, [commit]);
    assert_eq!(head.files["tasks/a.json"], canonical(&record));
    assert_eq!(pinned(&scenario), head.id.to_string());
    assert_clean(&library);
}

#[test]
fn after_a_kill_left_the_moved_pin_uncommitted_the_same_put_commits_it_and_then_commits_nothing() {
    let (library, commit) = library();
    let scenario = mounting(&library, commit);
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(".git/config", Item::Appended(IDENTITY));
    let record = task("");
    let text = String::from_utf8(canonical(&record)).unwrap();
    let killed = commit_in(&library, &[("tasks/a.json", &text)]);
    pin(&scenario, &library, killed);
    let report = put(&scenario, "/vendor/tasks/a", Expect::Any, &record).unwrap();
    let root = scenario.head().unwrap();
    assert_eq!(report.commit, Some(root.id));
    assert_eq!(
        root.files["vendor.json"],
        file(&scenario, "vendor.json").unwrap()
    );
    assert_eq!(pinned(&scenario), killed.to_string());
    assert_eq!(library.head().unwrap().id, killed);
    let again = put(&scenario, "/vendor/tasks/a", Expect::Any, &record).unwrap();
    assert_eq!(again.commit, None);
    assert_eq!(scenario.head().unwrap().id, root.id);
}

/// `library()` with its task schema at `schemas/task.json` and a symlink
/// whose text is `text` at the schema's path, committed and tracked.
fn linked_library(text: &str) -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let items: [(&str, Item<'_>); 5] = [
        (
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        ),
        ("schemas/task.json", Item::File(TASK_SCHEMA)),
        (".board/types/lr/task/v1.schema.json", Item::Symlink(text)),
        (
            ".board/types/lr/project/v1.schema.json",
            Item::File(PROJECT_SCHEMA),
        ),
        (
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        ),
    ];
    for (at, item) in items {
        library.write(at, item);
    }
    let bytes: Vec<(&[u8], Item<'_>)> = items
        .iter()
        .map(|(at, item)| (at.as_bytes(), *item))
        .collect();
    let commit = library.commit_items(&bytes);
    library
        .track(commit)
        .write(".git/config", Item::Appended(IDENTITY));
    (library, commit)
}

#[test]
fn a_put_through_a_mount_reads_a_schema_symlinked_to_a_file_and_one_to_nothing_is_refused() {
    let record = task(r#", "project": "/projects/p""#);
    let (library, commit) = linked_library("../../../../schemas/task.json");
    let scenario = mounting(&library, commit);
    put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap();
    let head = library.head().unwrap();
    assert_eq!(head.files["tasks/a.json"], canonical(&record));
    assert!(!open(&scenario).check(None, None).unwrap().degraded());

    let (library, commit) = linked_library("../../../../schemas/gone.json");
    let scenario = mounting(&library, commit);
    let refused = put(&scenario, "/vendor/tasks/a", Expect::Absent, &record).unwrap_err();
    assert_eq!(refused, Code::SymlinkDangling);
    assert_eq!(library.head().unwrap().id, commit);
}

#[test]
fn a_batch_through_a_mount_leaves_an_equal_schema_held_through_a_symlink_and_writes_its_record() {
    let (library, commit) = linked_library("../../../../schemas/task.json");
    let scenario = mounting(&library, commit);
    let record = batch_line(
        "/vendor/tasks/a",
        "record",
        &task(r#", "project": "/projects/p""#),
    );
    let report = put_batch(&scenario, &(task_schema_line() + &record)).unwrap();
    assert_eq!(report.writes.len(), 1, "only the record: {report:?}");
    let head = library.head().unwrap();
    assert_eq!(head.parents, [commit]);
    assert!(head.files.contains_key("tasks/a.json"));
    let kept = std::fs::read_link(
        library
            .workdir()
            .join(".board/types/lr/task/v1.schema.json"),
    );
    assert_eq!(
        kept.unwrap(),
        std::path::Path::new("../../../../schemas/task.json")
    );
}

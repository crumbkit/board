//! Writing records: checks, where they land, the lock and `--expect`.

use std::os::unix::fs::PermissionsExt;

use board_model::{Code, Fields, LogicalPath, ObjectId, canonical_json, parse_json};
use board_scenario::{Item, Scenario};
use board_tree::{LOCK_WAIT, Status};

use crate::put::{Expect, PutReport};
use crate::tests::{MANIFEST, open, task};

mod batch;

/// The test board, set not to commit: committing is tested on its own,
/// with an identity in the repository's own configuration.
fn board() -> Scenario {
    let scenario = crate::tests::board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    scenario
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn put(scenario: &Scenario, at: &str, expect: Expect, record: &str) -> Result<PutReport, Code> {
    open(scenario)
        .put(&path(at), expect, record.as_bytes())
        .map_err(|error| error.code())
}

fn file(scenario: &Scenario, physical: &str) -> Option<Vec<u8>> {
    std::fs::read(scenario.workdir().join(physical)).ok()
}

fn canonical(record: &str) -> Vec<u8> {
    canonical_json(&parse_json(record.as_bytes()).unwrap())
}

#[test]
fn a_valid_record_is_written_in_canonical_form_and_checks_clean() {
    let scenario = board();
    let record = task(r#", "project": "/projects/p""#);
    let report = put(&scenario, "/tasks/a", Expect::Any, &record).unwrap();
    assert_eq!(file(&scenario, "tasks/a.json").unwrap(), canonical(&record));
    let written = &report.writes[0];
    assert_eq!(written.location.path, "tasks/a.json");
    assert_eq!(written.display.to_string(), "/tasks/a");
    assert_eq!(written.previous, None);
    assert_eq!(
        written.blob,
        scenario.blob(std::str::from_utf8(&canonical(&record)).unwrap())
    );
    assert_eq!(report.commit, None);
    let checked = open(&scenario).check(None, None).unwrap();
    assert!(!checked.degraded());
    assert!(
        !scenario.workdir().join("tasks/.a.json.board-tmp").exists(),
        "no temporary file is left"
    );
}

#[test]
fn an_overwrite_reports_the_blob_it_replaced() {
    let scenario = board();
    let first = put(&scenario, "/tasks/a", Expect::Any, &task("")).unwrap();
    let second = put(
        &scenario,
        "/tasks/a",
        Expect::Any,
        &task(r#", "see": "/x""#),
    )
    .unwrap();
    assert_eq!(second.writes[0].previous, Some(first.writes[0].blob));
}

#[test]
fn expect_must_match_the_file_or_the_put_is_stale() {
    let scenario = board();
    let first = put(&scenario, "/tasks/a", Expect::Absent, &task(""))
        .unwrap()
        .writes[0]
        .blob;
    let other = ObjectId::Sha1([7; 20]);
    assert_eq!(
        put(&scenario, "/tasks/a", Expect::Blob(other), &task("")).unwrap_err(),
        Code::PutStale
    );
    assert_eq!(
        put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap_err(),
        Code::PutStale
    );
    assert!(
        put(
            &scenario,
            "/tasks/a",
            Expect::Blob(first),
            &task(", \"see\": \"/x\"")
        )
        .is_ok()
    );
}

#[test]
fn a_record_check_refuses_with_checks_own_codes_and_admits_its_twin() {
    let scenario = board();
    for (refused, code) in [
        (r#"{"$type": "lr/task/v1"}"#, Code::RecordInvalid),
        (r#"{"$type": "lr/nothing/v1"}"#, Code::RecordUnknownType),
        (r#"{"$type": "Not a type"}"#, Code::RecordBadType),
        (r#"{"$type": "board/other/v1"}"#, Code::RecordUnknownType),
        (
            r#"{"$type": "lr/task/v1", "title": "t", "see": "not a path"}"#,
            Code::RefMalformed,
        ),
        (r#"{"title": "t"}"#, Code::PutInvalid),
        ("[1]", Code::PutInvalid),
        ("", Code::JsonParse),
        (
            r#"{"$type": "lr/task/v1", "title": 18446744073709551616}"#,
            Code::JsonLossyNumber,
        ),
    ] {
        assert_eq!(
            put(&scenario, "/tasks/a", Expect::Any, refused).unwrap_err(),
            code,
            "{refused}"
        );
    }
    assert!(
        file(&scenario, "tasks/a.json").is_none(),
        "nothing refused was written"
    );
    assert!(
        put(
            &scenario,
            "/tasks/a",
            Expect::Any,
            &task(r#", "see": "/nowhere""#)
        )
        .is_ok()
    );
}

#[test]
fn a_refusal_names_what_is_wrong_with_the_record_and_its_twin_is_admitted() {
    let scenario = board();
    for (refused, code, message) in [
        (
            r#"{"$type": "lr/tsk/v1", "title": "t"}"#,
            Code::RecordUnknownType,
            "no schema for lr/tsk/v1 at .board/types/lr/tsk/v1.schema.json",
        ),
        (r#"{"title": "t"}"#, Code::PutInvalid, "it has no $type"),
        ("[1]", Code::PutInvalid, "it is not a json object"),
        (" \n", Code::JsonParse, "it is empty"),
    ] {
        let error = open(&scenario)
            .put(&path("/tasks/a"), Expect::Any, refused.as_bytes())
            .unwrap_err();
        assert_eq!(error.code(), code, "{refused}");
        assert_eq!(
            error.to_string(),
            format!("the record for /tasks/a is refused: {message}")
        );
    }
    assert!(
        put(
            &scenario,
            "/tasks/a",
            Expect::Any,
            r#"{"$type": "lr/task/v1", "title": "t"}"#
        )
        .is_ok()
    );
}

/// The fields of the refusal of `record` at `at`.
fn refused_fields(scenario: &Scenario, at: &str, expect: Expect, record: &str) -> Fields {
    open(scenario)
        .put(&path(at), expect, record.as_bytes())
        .unwrap_err()
        .fields()
}

#[test]
fn a_refused_record_names_its_path_and_what_is_wrong_and_its_twin_is_admitted() {
    let scenario = board();
    let at = Fields::at(&path("/tasks/a"));
    assert_eq!(
        refused_fields(
            &scenario,
            "/tasks/a",
            Expect::Any,
            r#"{"$type": "lr/tsk/v1"}"#
        ),
        Fields {
            record_type: Some("lr/tsk/v1".parse().unwrap()),
            schema: Some(".board/types/lr/tsk/v1.schema.json".to_owned()),
            ..at.clone()
        }
    );
    assert_eq!(
        refused_fields(
            &scenario,
            "/tasks/a",
            Expect::Any,
            r#"{"$type": "lr/task/v1"}"#
        ),
        Fields {
            pointer: Some(String::new()),
            ..at.clone()
        }
    );
    assert_eq!(
        refused_fields(&scenario, "/tasks/a", Expect::Any, &task(r#", "see": "x""#)),
        Fields {
            pointer: Some("/see".to_owned()),
            ..at.clone()
        }
    );
    assert_eq!(
        refused_fields(&scenario, "/tasks/a", Expect::Any, "[1]"),
        at
    );
    assert!(put(&scenario, "/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_stale_put_names_the_blob_its_path_holds_and_an_expect_that_matches_is_admitted() {
    let scenario = board();
    let first = put(&scenario, "/tasks/a", Expect::Absent, &task(""))
        .unwrap()
        .writes[0]
        .blob;
    assert_eq!(
        refused_fields(&scenario, "/tasks/a", Expect::Absent, &task("")),
        Fields {
            blob: Some(first),
            ..Fields::at(&path("/tasks/a"))
        }
    );
    assert_eq!(
        refused_fields(&scenario, "/tasks/b", Expect::Blob(first), &task("")),
        Fields::at(&path("/tasks/b"))
    );
    assert!(
        put(
            &scenario,
            "/tasks/a",
            Expect::Blob(first),
            &task(r#", "see": "/x""#)
        )
        .is_ok()
    );
}

#[test]
fn boards_own_types_are_checked_as_their_rust_types() {
    let scenario = board();
    assert_eq!(
        put(
            &scenario,
            "/links/l",
            Expect::Any,
            r#"{"$type": "board/mount/v1"}"#
        )
        .unwrap_err(),
        Code::MountInvalid
    );
    assert!(
        put(
            &scenario,
            "/links/l",
            Expect::Any,
            r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#
        )
        .is_ok()
    );
}

#[test]
fn a_write_through_a_link_lands_at_its_target() {
    let scenario = board();
    scenario.write(
        "links/l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
    );
    let record = r#"{"$type": "lr/project/v1", "name": "renamed"}"#;
    let report = put(&scenario, "/links/l", Expect::Any, record).unwrap();
    assert_eq!(
        file(&scenario, "projects/p.json").unwrap(),
        canonical(record)
    );
    assert_eq!(report.writes[0].display.to_string(), "/projects/p");
    assert!(
        file(&scenario, "links/l.json")
            .unwrap()
            .starts_with(b"{\"$type\": \"board/mount/v1\"")
    );
}

#[test]
fn a_write_through_a_mount_is_refused_and_one_beside_it_is_admitted() {
    let library = Scenario::new();
    let commit = library.commit(&[("x.json", "{}")]);
    let scenario = board();
    scenario.write(
        "vendor.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    assert_eq!(
        put(&scenario, "/vendor/x", Expect::Any, &task("")).unwrap_err(),
        Code::PutThroughMount
    );
    assert_eq!(
        put(&scenario, "/vendor/new/y", Expect::Any, &task("")).unwrap_err(),
        Code::PutThroughMount
    );
    assert_eq!(
        refused_fields(&scenario, "/vendor/x", Expect::Any, &task("")),
        Fields::at(&path("/vendor/x"))
    );
    assert!(put(&scenario, "/vendors/x", Expect::Any, &task("")).is_ok());
}

#[test]
fn missing_directories_are_created() {
    let scenario = board();
    put(&scenario, "/deep/er/a", Expect::Absent, &task("")).unwrap();
    assert_eq!(
        file(&scenario, "deep/er/a.json").unwrap(),
        canonical(&task(""))
    );
}

#[test]
fn a_record_gains_children_in_a_directory_beside_it() {
    let scenario = board();
    put(&scenario, "/projects/p/tasks/a", Expect::Absent, &task("")).unwrap();
    assert!(file(&scenario, "projects/p/tasks/a.json").is_some());
    assert!(file(&scenario, "projects/p.json").is_some());
}

#[test]
fn a_write_that_would_leave_its_path_degraded_is_refused_and_its_twin_is_admitted() {
    let scenario = board();
    scenario.write("tasks/x/child.json", Item::File(&task("")));
    let link = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    assert_eq!(
        put(&scenario, "/tasks/x", Expect::Any, link).unwrap_err(),
        Code::PathShadowed,
        "a pointer beside a directory shadows it"
    );
    assert!(put(&scenario, "/tasks/x", Expect::Any, &task("")).is_ok());
    assert_eq!(
        put(
            &scenario,
            "/Projects/q",
            Expect::Any,
            r#"{"$type": "lr/project/v1"}"#
        )
        .unwrap_err(),
        Code::PathCaseCollision
    );
    assert!(
        put(
            &scenario,
            "/projects/q",
            Expect::Any,
            r#"{"$type": "lr/project/v1"}"#
        )
        .is_ok()
    );
}

#[test]
fn a_path_that_names_a_file_that_is_not_a_record_is_refused() {
    let scenario = board();
    scenario.write("notes.md", Item::File("text"));
    assert_eq!(
        put(&scenario, "/notes.md", Expect::Any, &task("")).unwrap_err(),
        Code::RecordNotARecord
    );
    assert!(put(&scenario, "/notes", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_write_into_git_is_refused_under_any_spelling_and_its_twin_is_admitted() {
    let scenario = board();
    scenario.write("sub/x.json", Item::File(&task(""))).write(
        "links/sub.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/sub"}"#),
    );
    let cases = [
        (
            "/.git/hooks/x",
            ".git/hooks/x.json",
            "/git/hooks/x",
            "git/hooks/x.json",
        ),
        ("/.GIT/x", ".GIT/x.json", "/.GITS/x", ".GITS/x.json"),
        ("/sub/.git/y", "sub/.git", "/sub/gitx/y", "sub/gitx/y.json"),
        (
            "/sub/GIT~1/y",
            "sub/GIT~1",
            "/sub/GIT~2/y",
            "sub/GIT~2/y.json",
        ),
        (
            "/links/sub/.git/y",
            "sub/.git",
            "/links/sub/gity/y",
            "sub/gity/y.json",
        ),
    ];
    for (refused, unwritten, admitted, written) in cases {
        assert_eq!(
            put(&scenario, refused, Expect::Any, &task("")).unwrap_err(),
            Code::PathMalformed,
            "{refused}"
        );
        assert!(file(&scenario, unwritten).is_none(), "{unwritten}");
        assert!(!scenario.workdir().join(unwritten).exists(), "{unwritten}");
        put(&scenario, admitted, Expect::Any, &task("")).unwrap();
        assert!(file(&scenario, written).is_some(), "{written}");
    }
    // A link whose target is in `.git` names nothing: reads never see `.git`.
    scenario.write(
        "links/git.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/.git/hooks/z"}"#),
    );
    assert_eq!(
        put(&scenario, "/links/git", Expect::Any, &task("")).unwrap_err(),
        Code::RefDangling
    );
    assert!(!scenario.workdir().join(".git/hooks/z.json").exists());
}

#[test]
fn an_existing_file_keeps_its_permissions() {
    let scenario = board();
    put(&scenario, "/tasks/a", Expect::Any, &task("")).unwrap();
    let file = scenario.workdir().join("tasks/a.json");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    put(
        &scenario,
        "/tasks/a",
        Expect::Any,
        &task(r#", "see": "/x""#),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn a_held_lock_refuses_a_put_after_the_wait_and_a_released_one_admits_it() {
    let scenario = board();
    let board = open(&scenario);
    let held = board.repository().lock(LOCK_WAIT).unwrap();
    let started = std::time::Instant::now();
    let refused = board
        .put(&path("/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutLocked);
    assert!(started.elapsed() >= LOCK_WAIT);
    assert!(refused.to_string().contains("remove it"));
    drop(held);
    assert!(
        board
            .put(&path("/tasks/a"), Expect::Any, task("").as_bytes())
            .is_ok()
    );
}

#[test]
fn two_puts_with_the_same_expect_make_one_write_and_one_stale() {
    let scenario = board();
    let first = put(&scenario, "/tasks/a", Expect::Absent, &task(""))
        .unwrap()
        .writes[0]
        .blob;
    let results: Vec<Result<PutReport, Code>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|title| {
                let record = format!(r#"{{"$type": "lr/task/v1", "title": "{title}"}}"#);
                let root = scenario.workdir().to_path_buf();
                scope.spawn(move || {
                    crate::Board::open(&root, None)
                        .unwrap()
                        .put(&path("/tasks/a"), Expect::Blob(first), record.as_bytes())
                        .map_err(|error| error.code())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    let written = results.iter().filter(|result| result.is_ok()).count();
    let stale = results
        .iter()
        .filter(|result| **result == Err(Code::PutStale))
        .count();
    assert_eq!((written, stale), (1, 1));
    let checked = open(&scenario).check(None, None).unwrap();
    assert!(
        checked
            .entries
            .iter()
            .all(|entry| entry.status == Status::Ok)
    );
}

/// The test board, committing on put, with an identity in the repository's
/// own configuration and an uncommitted note beside the records.
fn committing() -> Scenario {
    let scenario = crate::tests::board();
    scenario
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        )
        .write("notes.md", Item::File("not committed"));
    scenario
}

#[test]
fn a_put_commits_only_the_paths_it_wrote_onto_head() {
    let scenario = committing();
    let report = put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap();
    let head = scenario.head().unwrap();
    assert_eq!(Some(head.id), report.commit);
    assert_eq!(head.message, "board put /tasks/a");
    assert_eq!(head.parents, []);
    assert_eq!(
        head.files.keys().collect::<Vec<_>>(),
        ["tasks/a.json"],
        "the uncommitted note and records stay out"
    );
    assert_eq!(head.files["tasks/a.json"], canonical(&task("")));
    assert_eq!(
        scenario.index().get("tasks/a.json"),
        Some(&report.writes[0].blob),
        "git's index holds what was committed"
    );
    let second = put(
        &scenario,
        "/tasks/a",
        Expect::Any,
        &task(r#", "see": "/x""#),
    )
    .unwrap();
    assert_eq!(scenario.head().unwrap().parents, [report.commit.unwrap()]);
    assert_eq!(scenario.head().unwrap().id, second.commit.unwrap());
}

#[test]
fn a_committed_put_checks_as_committed_and_the_rest_as_before() {
    let scenario = committing();
    put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap();
    let report = open(&scenario).check(None, None).unwrap();
    let change = |path: &str| {
        report
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap()
            .change
    };
    assert_eq!(change("tasks/a.json"), board_model::Change::Committed);
    assert_eq!(change("notes.md"), board_model::Change::Uncommitted);
}

#[test]
fn a_held_git_index_lock_refuses_a_committing_put_before_it_writes() {
    let scenario = committing();
    scenario.write(".git/index.lock", Item::File(""));
    assert_eq!(
        put(&scenario, "/tasks/a", Expect::Any, &task("")).unwrap_err(),
        Code::PutLocked
    );
    assert!(file(&scenario, "tasks/a.json").is_none());
    assert!(scenario.head().is_none());
    scenario.write(".git/index.lock", Item::Deleted);
    assert!(put(&scenario, "/tasks/a", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_link_whose_target_is_missing_is_refused_and_one_whose_target_exists_is_followed() {
    let scenario = board();
    scenario
        .write(
            "links/gone.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/gone"}"#),
        )
        .write(
            "links/here.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        );
    let project = r#"{"$type": "lr/project/v1"}"#;
    assert_eq!(
        put(&scenario, "/links/gone", Expect::Any, project).unwrap_err(),
        Code::RefDangling
    );
    assert!(
        file(&scenario, "projects/gone.json").is_none(),
        "put does not create a link's target"
    );
    assert!(put(&scenario, "/links/here", Expect::Any, project).is_ok());
}

#[test]
fn put_refuses_a_broken_plugin_manifest_and_writes_a_valid_one() {
    let scenario = board();
    let broken = MANIFEST.replace(r#"["./run"]"#, "[]");
    let refused = open(&scenario)
        .put(&path("/plugins/p/plugin"), Expect::Any, broken.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PluginBadManifest);
    assert!(refused.to_string().contains("empty run"), "{refused}");
    assert!(put(&scenario, "/plugins/p/plugin", Expect::Any, MANIFEST).is_ok());
}

//! Writing records: checks, where they land, the lock and `--expect`.

use std::os::unix::fs::PermissionsExt;

use board_model::{Code, LogicalPath, ObjectId, canonical_json, parse_json};
use board_scenario::{Item, Scenario};
use board_tree::{LOCK_WAIT, Status};

use crate::put::{Expect, PutReport};
use crate::tests::{board, open, task};

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

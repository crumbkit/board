//! Writing records: checks, where they land, the lock and `--expect`.

use std::os::unix::fs::PermissionsExt;

use board_model::{
    Code, Fields, Found, Lined, LogicalPath, ObjectId, RefusalCode, canonical_json, parse_json,
};
use board_scenario::{Item, Scenario};
use board_tree::{GitError, LOCK_WAIT, ResolveError, Status};
use serde_json::{Value, json};

use crate::Board;
use crate::Detail;
use crate::put::{Expect, PutReport, Schemas};
use crate::tests::{MANIFEST, open, task};

mod batch;
mod parity;
mod waiting;

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
fn expect_over_a_file_too_large_to_read_compares_its_whole_blob() {
    let scenario = board();
    let big = "a".repeat(board_model::MAX_RECORD_BYTES + 1);
    scenario.write("tasks/a.json", Item::File(&big));
    assert_eq!(
        put(&scenario, "/tasks/a", Expect::Absent, &task("")).unwrap_err(),
        Code::PutStale
    );
    let written = put(
        &scenario,
        "/tasks/a",
        Expect::Blob(scenario.blob(&big)),
        &task(""),
    );
    assert_eq!(
        written.unwrap().writes[0].previous,
        Some(scenario.blob(&big))
    );
}

#[test]
fn an_ignored_file_too_large_to_read_is_never_absent_and_its_own_blob_is_expected() {
    let scenario = board();
    let big = "a".repeat(board_model::MAX_RECORD_BYTES + 1);
    scenario
        .write(".gitignore", Item::File("tasks/r/a.json\n"))
        .write("tasks/r.json", Item::File(&task("")))
        .write("tasks/r/a.json", Item::File(&big));
    let beside = scenario.blob(&task(""));
    for expect in [Expect::Absent, Expect::Blob(beside)] {
        let refused = put(&scenario, "/tasks/r/a", expect, &task(""));
        assert_eq!(refused.unwrap_err(), Code::PutStale);
    }
    let own = scenario.blob(&big);
    let written = put(&scenario, "/tasks/r/a", Expect::Blob(own), &task("")).unwrap();
    assert_eq!(written.writes[0].previous, Some(own));
}

#[test]
fn expect_over_a_symlink_is_refused_whichever_blob_it_names_and_over_the_target_is_admitted() {
    let scenario = board();
    scenario
        .write("tasks/b.json", Item::File(&task("")))
        .write("tasks/a.json", Item::Symlink("b.json"));
    let targets = scenario.blob(&task(""));
    let links = scenario.blob("b.json");
    for expect in [Expect::Blob(targets), Expect::Blob(links)] {
        assert_eq!(
            put(&scenario, "/tasks/a", expect, &task("")).unwrap_err(),
            Code::PutSymlink
        );
    }
    let written = put(&scenario, "/tasks/b", Expect::Blob(targets), &task("")).unwrap();
    assert_eq!(written.writes[0].previous, Some(targets));
}

#[test]
fn a_record_whose_schema_is_a_symlink_is_refused_as_check_reads_it_and_a_regular_twin_is_admitted()
{
    let scenario = board();
    scenario
        .write("schemas/note.json", Item::File(r#"{"type": "object"}"#))
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::Symlink("../../../../schemas/note.json"),
        );
    let note = r#"{"$type": "lr/note/v1"}"#;
    assert_eq!(
        put(&scenario, "/n", Expect::Any, note).unwrap_err(),
        Code::SchemaInvalid
    );
    scenario
        .write(".board/types/lr/note/v1.schema.json", Item::Deleted)
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        );
    assert!(put(&scenario, "/n", Expect::Any, note).is_ok());
}

/// The codes put shares with check are pinned by the property in `parity`;
/// these are put's own checks, made before it reads the record as check does.
#[test]
fn input_that_is_not_one_object_with_a_type_is_refused_and_its_twin_is_admitted() {
    let scenario = board();
    for (refused, code) in [
        (r#"{"title": "t"}"#, Code::PutInvalid),
        ("[1]", Code::PutInvalid),
        ("", Code::JsonParse),
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
    // A code alone says what is wrong, so the message adds nothing.
    let config = |fields: &str| format!(r#"{{"$type": "board/config/v1"{fields}}}"#);
    let error = open(&scenario)
        .put(
            &path("/tasks/a"),
            Expect::Any,
            config(r#", "colour": "red""#).as_bytes(),
        )
        .unwrap_err();
    assert_eq!(error.code(), Code::ConfigInvalid);
    assert_eq!(error.to_string(), "the record for /tasks/a is refused");
    assert!(put(&scenario, "/tasks/c", Expect::Any, &config("")).is_ok());
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

#[test]
fn a_record_its_schema_refuses_is_named_once_as_the_subject_and_its_twin_is_admitted() {
    let scenario = board();
    for (refused, code, message) in [
        (
            r#"{"$type": "lr/task/v1", "title": 5}"#.to_owned(),
            Code::RecordInvalid,
            r#"the record for /tasks/a fails its schema at "/title": 5 is not of type "string""#,
        ),
        (
            r#"{"$type": "lr/task/v1"}"#.to_owned(),
            Code::RecordInvalid,
            r#"the record for /tasks/a fails its schema at its root: "title" is a required property"#,
        ),
        (
            task(r#", "see": "a//b""#),
            Code::RefMalformed,
            r#"the record for /tasks/a holds a reference at "/see": "a//b" is not a logical path"#,
        ),
    ] {
        let error = open(&scenario)
            .put(&path("/tasks/a"), Expect::Any, refused.as_bytes())
            .unwrap_err();
        assert_eq!(error.code(), code, "{refused}");
        assert_eq!(error.to_string(), message);
    }
    assert!(
        put(
            &scenario,
            "/tasks/a",
            Expect::Any,
            &task(r#", "see": "/b""#)
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
            found: Some(Found::Blob(first)),
            ..Fields::at(&path("/tasks/a"))
        }
    );
    assert_eq!(
        refused_fields(&scenario, "/tasks/b", Expect::Blob(first), &task("")),
        Fields {
            found: Some(Found::Nothing),
            ..Fields::at(&path("/tasks/b"))
        }
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
    let commit = library.commit(&[("x.json", "{}"), ("notes.md", "x")]);
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
    for refused in [
        "/vendor/new/y",
        "/vendor/notes.md",
        "/vendor/notes.md/a",
        "/vendor",
    ] {
        assert_eq!(
            put(&scenario, refused, Expect::Any, &task("")).unwrap_err(),
            Code::PutThroughMount,
            "{refused}"
        );
    }
    assert_eq!(
        refused_fields(&scenario, "/vendor/x", Expect::Any, &task("")),
        Fields::at(&path("/vendor/x"))
    );
    assert!(put(&scenario, "/vendors/x", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_write_beneath_a_mount_whose_commit_is_missing_names_the_mount_and_its_twin_is_admitted() {
    let library = Scenario::new();
    let commit = library.commit(&[("x.json", "{}")]);
    let scenario = board();
    let mount = |commit: ObjectId| {
        format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )
    };
    scenario.write("vendor.json", Item::File(&mount(ObjectId::Sha1([9; 20]))));
    let error = open(&scenario)
        .put(&path("/vendor/x"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(error.code(), Code::MountCommitMissing);
    assert_eq!(error.fields().path, Some(path("/vendor")));
    assert_eq!(
        error.code().hint(&error.fields()),
        "fetch the commit into the mounted repository, or run `board mount update /vendor`"
    );
    scenario.write("vendor.json", Item::File(&mount(commit)));
    assert_eq!(
        put(&scenario, "/vendor/x", Expect::Any, &task("")).unwrap_err(),
        Code::PutThroughMount
    );
    assert!(put(&scenario, "/vendors/x", Expect::Any, &task("")).is_ok());
}

#[test]
fn a_write_through_a_cycle_of_links_says_so_and_its_twin_is_admitted() {
    let scenario = board();
    let link = |to: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{to}"}}"#);
    scenario
        .write("a.json", Item::File(&link("/b")))
        .write("b.json", Item::File(&link("/a")))
        .write("c/y.json", Item::File(&task("")));
    let error = open(&scenario)
        .put(&path("/a/x"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(error.code(), Code::ResolveCycle);
    assert_eq!(
        error.to_string(),
        "/a/x cannot be written: it returns to a board and path it already resolved through a pointer"
    );
    assert_eq!(error.fields(), Fields::at(&path("/a/x")));
    scenario.write("b.json", Item::File(&link("/c")));
    assert!(put(&scenario, "/a/x", Expect::Any, &task("")).is_ok());
    assert!(file(&scenario, "c/x.json").is_some());
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
fn a_put_through_an_ignored_symlink_is_refused_and_through_an_ignored_directory_is_admitted() {
    let scenario = board();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().to_str().unwrap();
    scenario
        .write(".gitignore", Item::File("x\n"))
        .write("tasks/x", Item::Symlink(outside_path));
    let refused = open(&scenario)
        .put(&path("/tasks/x/y"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutSymlink);
    assert_eq!(
        refused.to_string(),
        "/tasks/x/y cannot be written: tasks/x is a symlink"
    );
    assert!(!outside.path().join("y.json").exists());
    scenario
        .write("tasks/x", Item::Deleted)
        .write("tasks/x/z.md", Item::File(""));
    put(&scenario, "/tasks/x/y", Expect::Any, &task("")).unwrap();
    assert_eq!(
        file(&scenario, "tasks/x/y.json").unwrap(),
        canonical(&task(""))
    );
}

#[test]
fn a_put_over_a_symlink_named_as_a_record_is_refused_and_over_a_file_is_admitted() {
    let scenario = board();
    scenario.write("s.json", Item::Symlink("tasks"));
    assert_eq!(
        put(&scenario, "/s", Expect::Any, &task("")).unwrap_err(),
        Code::PutSymlink
    );
    assert!(
        std::fs::symlink_metadata(scenario.workdir().join("s.json"))
            .unwrap()
            .is_symlink()
    );
    scenario
        .write("s.json", Item::Deleted)
        .write("s.json", Item::File(&task("")));
    put(&scenario, "/s", Expect::Any, &task("")).unwrap();
}

#[test]
fn a_put_through_a_listed_symlink_is_refused_naming_it_and_through_a_directory_is_admitted() {
    let scenario = board();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().to_str().unwrap();
    let tracked = scenario.commit_items(&[(b"outside", Item::Symlink(outside_path))]);
    scenario
        .track(tracked)
        .write("outside", Item::Symlink(outside_path))
        .write("sym", Item::Symlink("tasks"))
        .write("dangling", Item::Symlink("nowhere"))
        .write("tasks/inner", Item::Symlink(outside_path));
    let cases = [
        ("/sym/a", "sym"),
        ("/sym/deep/a", "sym"),
        ("/sym", "sym"),
        ("/outside/a", "outside"),
        ("/tasks/inner/a", "tasks/inner"),
        ("/dangling/a", "dangling"),
    ];
    for (at, symlink) in cases {
        let refused = open(&scenario)
            .put(&path(at), Expect::Any, task("").as_bytes())
            .unwrap_err();
        assert_eq!(refused.code(), Code::PutSymlink, "{at}");
        assert_eq!(
            refused.to_string(),
            format!("{at} cannot be written: {symlink} is a symlink")
        );
    }
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    for symlink in ["sym", "outside", "tasks/inner", "dangling"] {
        scenario
            .write(symlink, Item::Deleted)
            .write(&format!("{symlink}/z.md"), Item::File(""));
    }
    for (at, _) in cases {
        put(&scenario, at, Expect::Any, &task("")).unwrap();
    }
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
    let error = open(&scenario)
        .put(
            &path("/Projects/q"),
            Expect::Any,
            br#"{"$type": "lr/project/v1"}"#,
        )
        .unwrap_err();
    assert_eq!(error.code(), Code::PathCaseCollision);
    assert_eq!(
        error.to_string(),
        "/Projects/q cannot be written: the directory Projects it would create would differ only in case from projects"
    );
    assert_eq!(error.fields().directory.as_deref(), Some("Projects"));
    assert!(file(&scenario, "Projects/q.json").is_none());
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

/// Only a file system that tells case apart holds `tasks/` and `Tasks/`.
#[cfg(target_os = "linux")]
#[test]
fn a_write_beneath_a_directory_that_collides_in_case_is_refused_naming_it_and_its_twin_is_admitted()
{
    let scenario = board();
    scenario.write("tasks/a.json", Item::File(&task("")));
    assert!(put(&scenario, "/tasks/b", Expect::Any, &task("")).is_ok());
    scenario.write("Tasks/a.json", Item::File(&task("")));
    let error = open(&scenario)
        .put(&path("/tasks/c"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(error.code(), Code::PathCaseCollision);
    assert_eq!(
        error.to_string(),
        "/tasks/c cannot be written: the directory tasks above it differs only in case from Tasks"
    );
    assert!(file(&scenario, "tasks/c.json").is_none());
}

#[test]
fn a_path_that_names_a_file_that_is_not_a_record_is_refused() {
    let scenario = board();
    scenario.write("notes.md", Item::File("text"));
    assert_eq!(
        put(&scenario, "/notes.md", Expect::Any, &task("")).unwrap_err(),
        Code::RecordNotARecord
    );
    let error = open(&scenario)
        .put(&path("/notes.md/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(error.code(), Code::RecordNotARecord);
    assert_eq!(
        error.to_string(),
        "/notes.md/a cannot be written: /notes.md is not a record"
    );
    assert_eq!(
        error.code().hint(&error.fields()),
        "only a record has a JSON report or can be put; `board ls /notes.md` shows what is there"
    );
    assert!(put(&scenario, "/notes", Expect::Any, &task("")).is_ok());
    assert!(put(&scenario, "/notes/a", Expect::Any, &task("")).is_ok());
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
fn a_held_lock_refuses_a_put_and_a_released_one_admits_it() {
    let scenario = board();
    let board = open(&scenario);
    let held = board.repository().lock(LOCK_WAIT).unwrap();
    let refused = board
        .put(&path("/tasks/a"), Expect::Any, task("").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutLocked);
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
fn a_write_left_degraded_says_where_and_why_as_check_does_and_its_twin_is_clean() {
    let scenario = board();
    let degraded = |at: &str, record: &str| {
        put(&scenario, at, Expect::Any, record).unwrap().writes[0]
            .degraded
            .clone()
    };
    let dangling = degraded(
        "/tasks/b",
        &task(r#", "project": "/nowhere", "see": "/gone""#),
    );
    let wrong = degraded("/tasks/c", &task(r#", "project": "/tasks/b""#));
    let link = degraded(
        "/links/l",
        r#"{"$type": "board/mount/v1", "path": "/gone"}"#,
    );
    assert_eq!(
        degraded("/tasks/a", &task(r#", "project": "/projects/p""#)),
        None
    );
    let detail = |pointer: Option<&str>, reason: &str, failures| Detail {
        pointer: pointer.map(str::to_owned),
        reason: reason.to_owned(),
        failures,
        at: None,
        mount: None,
    };
    assert_eq!(
        dangling,
        Some((
            Code::RefDangling,
            detail(
                Some("/project"),
                r#"holds a reference at "/project": nothing is at /nowhere"#,
                2
            )
        ))
    );
    assert_eq!(
        wrong,
        Some((
            Code::RefWrongType,
            detail(
                Some("/project"),
                r#"holds a reference at "/project": /tasks/b is not a record of type lr/project/v1"#,
                1
            )
        ))
    );
    assert_eq!(
        link,
        Some((
            Code::RefDangling,
            Detail {
                at: Some("/links/l".parse().unwrap()),
                ..detail(
                    None,
                    "does not resolve: the link at /links/l points to /gone, which is missing",
                    1
                )
            }
        ))
    );
    let checked = open(&scenario).check(None, None).unwrap();
    for (physical, put) in [
        ("tasks/b.json", dangling),
        ("tasks/c.json", wrong),
        ("links/l.json", link),
    ] {
        let entry = checked
            .entries
            .iter()
            .find(|entry| entry.path == physical)
            .unwrap();
        let (code, detail) = put.unwrap();
        assert_eq!(entry.status, Status::Degraded(code));
        assert_eq!(entry.detail, Some(detail), "{physical}");
    }
}

#[test]
fn a_batch_reference_to_a_later_line_is_not_degraded() {
    let scenario = board();
    let input = format!(
        "{}\n{}\n",
        r#"{"path": "/tasks/a", "record": {"$type": "lr/task/v1", "title": "t", "see": "/tasks/b"}}"#,
        r#"{"path": "/tasks/b", "record": {"$type": "lr/task/v1", "title": "t"}}"#,
    );
    let report = open(&scenario).put_batch(&mut input.as_bytes()).unwrap();
    assert_eq!(report.writes[0].degraded, None);
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

#[test]
fn a_write_left_with_no_view_reports_the_error_at_its_pointer_or_each_reference() {
    let scenario = board();
    let board = open(&scenario);
    let lock = board.repository().lock(LOCK_WAIT).unwrap();
    let mut view = board.repositories().snapshot().unwrap();
    let mut schemas = Schemas::default();
    let mut prepare = |at: &str, record: &str| {
        let bytes = record.as_bytes();
        board
            .prepare(
                &lock,
                &mut view,
                &mut schemas,
                &path(at),
                Expect::Any,
                bytes,
            )
            .unwrap()
    };
    let link = prepare(
        "/links/l",
        r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#,
    );
    let referring = prepare("/tasks/a", &task(r#", "see": "/tasks/b", "project": "/x""#));
    let plain = prepare("/tasks/c", &task(""));
    let error = ResolveError::Git(GitError::NoWorkingTree {
        repository: scenario.workdir().to_path_buf(),
    });
    let reason = error.to_string();
    let (code, detail) = Board::unresolvable(&link, &error).unwrap();
    assert_eq!(
        (code, detail.pointer, detail.failures),
        (error.code(), None, 1)
    );
    assert_eq!(detail.reason, format!("does not resolve: {reason}"));
    let (code, detail) = Board::unresolvable(&referring, &error).unwrap();
    assert_eq!((code, detail.failures), (Code::RefDangling, 2));
    assert_eq!(
        detail.reason,
        format!(r#"holds a reference at "/project": {reason}"#)
    );
    assert_eq!(Board::unresolvable(&plain, &error), None);
}

#[test]
fn a_stale_put_refusal_names_its_path_and_what_it_found_there() {
    let blob = ObjectId::Sha1([1; 20]);
    let stale = |found| {
        crate::PutError::Stale {
            path: path("/tasks/a"),
            found,
        }
        .refusal()
    };
    let held = stale(Some(blob));
    assert_eq!(held.code, RefusalCode::Board(Code::PutStale));
    assert_eq!(
        held.message,
        format!("/tasks/a is not what --expect says: it holds {blob}")
    );
    assert_eq!(
        Value::Object(held.fields),
        json!({"path": "/tasks/a", "blob": blob.to_string(), "found": blob.to_string()})
    );
    assert!(held.hint.unwrap().ends_with(&format!("--expect {blob}`")));
    let nothing = stale(None);
    assert_eq!(
        Value::Object(nothing.fields),
        json!({"path": "/tasks/a", "found": null})
    );
    assert!(nothing.hint.unwrap().ends_with("--expect none`"));
}

#[test]
fn a_batch_refusal_lists_each_line_with_its_own_fields_and_a_single_refusal_lists_none() {
    let record = crate::PutError::Record {
        path: path("/tasks/a"),
        refused: crate::Refused::NoType,
    };
    let single = record.refusal();
    assert_eq!(single.code, RefusalCode::Board(Code::PutInvalid));
    assert_eq!(
        single.message,
        "the record for /tasks/a is refused: it has no $type"
    );
    assert_eq!(Value::Object(single.fields), json!({"path": "/tasks/a"}));
    let batch = crate::PutError::Batch {
        code: Code::PutInvalid,
        refusals: vec![Lined::new(3, record.code(), &record, record.fields())],
    }
    .refusal();
    assert_eq!(
        batch.lines,
        [Lined::new(3, record.code(), &record, record.fields())]
    );
    assert_eq!(batch.code, RefusalCode::Board(Code::PutInvalid));
    assert_eq!(batch.message, "the batch is refused");
    assert_eq!(
        Value::Object(batch.fields),
        json!({"refusals": [{
            "line": 3,
            "code": "board::put::invalid",
            "message": "the record for /tasks/a is refused: it has no $type",
            "fields": {"path": "/tasks/a"},
        }]})
    );
    assert_eq!(
        batch.hint.as_deref(),
        Some("fix each line listed, then send the whole batch again; nothing was written")
    );
}

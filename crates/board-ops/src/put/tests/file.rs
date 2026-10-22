//! `put --file`: bytes written as given at a path that is the file's whole
//! name, with no check beyond the size limit.

use board_model::{Code, Fields, MAX_RECORD_BYTES};
use board_scenario::{Item, Scenario};

use super::{board, file, path, put};
use crate::put::{Expect, PutError, PutReport};
use crate::tests::{open, task};

fn put_file(
    scenario: &Scenario,
    at: &str,
    expect: Expect,
    bytes: &[u8],
) -> Result<PutReport, PutError> {
    open(scenario).put_file(&path(at), expect, bytes)
}

fn code(result: Result<PutReport, PutError>) -> Code {
    result.unwrap_err().code()
}

#[test]
fn a_file_is_written_as_given_at_its_whole_name_and_an_overwrite_names_the_blob_it_replaced() {
    let scenario = board();
    let report = put_file(
        &scenario,
        "/reports/r1/notes.md",
        Expect::Absent,
        b"# Notes\n",
    )
    .unwrap();
    assert_eq!(
        file(&scenario, "reports/r1/notes.md").unwrap(),
        b"# Notes\n"
    );
    let written = &report.writes[0];
    assert_eq!(written.location.path, "reports/r1/notes.md");
    assert_eq!(written.display.to_string(), "/reports/r1/notes.md");
    assert_eq!(written.previous, None);
    assert_eq!(written.blob, scenario.blob("# Notes\n"));
    assert_eq!(written.degraded, None);
    assert_eq!(report.commit, None);
    let again = put_file(
        &scenario,
        "/reports/r1/notes.md",
        Expect::Blob(written.blob),
        b"# Notes\n\nMore.\n",
    )
    .unwrap();
    assert_eq!(again.writes[0].previous, Some(written.blob));
    assert_eq!(
        code(put_file(
            &scenario,
            "/reports/r1/notes.md",
            Expect::Absent,
            b""
        )),
        Code::PutStale
    );
    let checked = open(&scenario).check(None, None).unwrap();
    assert!(!checked.degraded());
}

#[test]
fn a_file_at_a_json_name_a_record_or_a_directory_is_refused_and_one_at_its_whole_name_is_admitted()
{
    let scenario = board();
    put(&scenario, "/tasks/a", Expect::Any, &task("")).unwrap();
    for refused in ["/notes.json", "/tasks/a", "/tasks", "/projects", "/"] {
        assert_eq!(
            code(put_file(&scenario, refused, Expect::Any, b"x")),
            Code::PutNotAFile,
            "{refused}"
        );
    }
    let record = put_file(&scenario, "/tasks/a", Expect::Any, b"x").unwrap_err();
    assert_eq!(
        record.to_string(),
        "/tasks/a cannot hold a file: tasks/a.json is read as a record"
    );
    assert_eq!(record.fields(), Fields::at(&path("/tasks/a")));
    assert_eq!(
        record.code().hint(&record.fields()),
        "put a file with `--file` at its whole name, such as `/notes/plan.md`; a name ending `.json` is read as a record, and a directory holds no bytes"
    );
    let directory = put_file(&scenario, "/tasks", Expect::Any, b"x").unwrap_err();
    assert_eq!(
        directory.to_string(),
        "/tasks cannot hold a file: it is a directory"
    );
    assert!(put_file(&scenario, "/notes.md", Expect::Any, b"x").is_ok());
    assert!(put_file(&scenario, "/tasks/a.md", Expect::Any, b"x").is_ok());
    assert!(put_file(&scenario, "/tasks/a/b.md", Expect::Any, b"x").is_ok());
}

#[test]
fn a_file_beneath_a_file_is_refused_as_a_record_beneath_one_is() {
    let scenario = board();
    scenario.write("notes.md", Item::File("text"));
    let error = put_file(&scenario, "/notes.md/a.md", Expect::Any, b"x").unwrap_err();
    assert_eq!(error.code(), Code::RecordNotARecord);
    assert_eq!(
        error.to_string(),
        "/notes.md/a.md cannot be written: /notes.md is not a record"
    );
    assert_eq!(
        error.code().hint(&error.fields()),
        "only a record is put without `--file`, and nothing is put beneath a file; `board ls /notes.md` shows what is there"
    );
}

#[test]
fn a_file_through_a_mount_is_refused_and_one_beside_it_is_admitted() {
    let library = Scenario::new();
    let commit = library.commit(&[("notes.md", "x")]);
    let scenario = board();
    scenario.write(
        "vendor.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )),
    );
    for refused in ["/vendor/notes.md", "/vendor/new.md"] {
        let error = put_file(&scenario, refused, Expect::Any, b"x").unwrap_err();
        assert_eq!(error.code(), Code::PutThroughMount, "{refused}");
        assert_eq!(error.fields(), Fields::at(&path("/vendor")));
    }
    assert!(put_file(&scenario, "/vendors/notes.md", Expect::Any, b"x").is_ok());
}

#[test]
fn a_file_over_the_size_limit_is_refused_and_one_at_the_limit_is_admitted() {
    let scenario = board();
    let bytes = vec![b'x'; MAX_RECORD_BYTES + 1];
    let error = put_file(&scenario, "/big.bin", Expect::Any, &bytes).unwrap_err();
    assert_eq!(error.code(), Code::JsonTooLarge);
    assert_eq!(
        error.to_string(),
        format!(
            "/big.bin cannot be written: {} bytes is over {MAX_RECORD_BYTES}",
            bytes.len()
        )
    );
    assert!(put_file(&scenario, "/big.bin", Expect::Any, &bytes[1..]).is_ok());
}

/// The code `put --file` reports `text` degraded with at `at`.
fn degraded(scenario: &Scenario, at: &str, text: &str) -> Option<Code> {
    let report = put_file(scenario, at, Expect::Any, text.as_bytes()).unwrap();
    report.writes[0].degraded.as_ref().map(|(code, _)| *code)
}

#[test]
fn a_note_with_a_link_to_nothing_is_reported_degraded_and_its_twin_is_not() {
    let scenario = board();
    assert_eq!(
        degraded(&scenario, "/notes/a.md", "[gone](gone.md)"),
        Some(Code::RefDangling)
    );
    assert_eq!(
        degraded(&scenario, "/notes/a.md", "[p](../projects/p.json)"),
        None
    );
}

#[test]
fn a_note_written_through_a_link_reads_its_links_from_where_it_lands() {
    let scenario = board();
    scenario
        .write(
            "alias.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/deep/dir"}"#),
        )
        .write("deep/x.md", Item::File("here"))
        .write("deep/dir/other.md", Item::File("beside"));
    // From /deep/dir/note.md, ../x.md is /deep/x.md; from /alias/note.md it
    // would be /x.md, which is nothing.
    let checked = || {
        open(&scenario)
            .check(None, None)
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.path == "deep/dir/note.md")
            .unwrap()
            .status
    };
    assert_eq!(degraded(&scenario, "/alias/note.md", "[x](../x.md)"), None);
    assert_eq!(checked(), board_tree::Status::Ok);
    assert_eq!(
        degraded(&scenario, "/alias/note.md", "[x](../../x.md)"),
        Some(Code::RefDangling)
    );
    assert_eq!(checked(), board_tree::Status::Degraded(Code::RefDangling));
}

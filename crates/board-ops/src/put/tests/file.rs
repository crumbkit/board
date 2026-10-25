//! `put --file`: bytes written as given at a path that is the file's whole
//! name, checked for size and, in a Markdown file, for frontmatter that
//! parses and a record its schema admits.

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

#[test]
fn a_text_holding_nul_is_refused_naming_its_offset_and_the_same_text_without_it_is_admitted() {
    let scenario = board();
    let error = put_file(&scenario, "/notes/a.md", Expect::Any, b"ab\0c").unwrap_err();
    assert_eq!(error.code(), Code::PutNul);
    assert_eq!(
        error.fields(),
        Fields {
            offset: Some(2),
            ..Fields::at(&path("/notes/a.md"))
        }
    );
    assert_eq!(
        error.code().hint(&error.fields()),
        "remove the NUL byte at byte offset 2 and put the text again"
    );
    assert_eq!(file(&scenario, "notes/a.md"), None);
    assert!(put_file(&scenario, "/notes/a.md", Expect::Any, b"abc").is_ok());
}

#[test]
fn bytes_that_are_not_utf8_may_hold_nul_and_the_same_bytes_as_text_may_not() {
    let scenario = board();
    let text = put_file(&scenario, "/data/a.bin", Expect::Any, b"a\0b").unwrap_err();
    assert_eq!(text.code(), Code::PutNul);
    assert!(put_file(&scenario, "/data/a.bin", Expect::Any, b"a\0b\xff").is_ok());
}

#[test]
fn a_file_that_already_holds_nul_is_indexed_and_not_degraded() {
    let scenario = board();
    scenario.write("notes/a.md", Item::File("ab\0c"));
    let checked = open(&scenario).check(None, None).unwrap();
    let entry = checked
        .entries
        .iter()
        .find(|entry| entry.path == "notes/a.md")
        .unwrap();
    assert_eq!(entry.status, board_tree::Status::Ok);
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

#[test]
fn a_markdown_record_its_schema_refuses_is_not_written_and_its_valid_twin_is() {
    let scenario = board();
    let error = put_file(
        &scenario,
        "/tasks/a.md",
        Expect::Any,
        b"---\n$type: lr/task/v1\ntitle: 1\n---\n# A\n",
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    assert_eq!(file(&scenario, "tasks/a.md"), None);
    let twin = b"---\n$type: lr/task/v1\ntitle: A\n---\n# A\n";
    let report = put_file(&scenario, "/tasks/a.md", Expect::Any, twin).unwrap();
    assert_eq!(report.writes[0].degraded, None);
    assert_eq!(file(&scenario, "tasks/a.md").unwrap(), twin);
}

#[test]
fn typed_frontmatter_that_does_not_parse_is_not_written_and_each_twin_is() {
    let scenario = board();
    let typed = |rest: &str| format!("---\n$type: lr/task/v1\n{rest}---\n");
    for (at, refused, code_of, admitted) in [
        (
            "/a.md",
            "title: [\n",
            Code::FrontmatterParse,
            "title: '['\n",
        ),
        (
            "/b.md",
            "title: a\ntitle: b\n",
            Code::FrontmatterParse,
            "title: a\n",
        ),
        (
            "/c.md",
            "title: .nan\n",
            Code::FrontmatterUnsupported,
            "title: '.nan'\n",
        ),
    ] {
        let (refused, admitted) = (typed(refused), typed(admitted));
        assert_eq!(
            code(put_file(&scenario, at, Expect::Any, refused.as_bytes())),
            code_of,
            "{refused}"
        );
        assert_eq!(file(&scenario, &at[1..]), None);
        assert!(
            put_file(&scenario, at, Expect::Any, admitted.as_bytes()).is_ok(),
            "{admitted}"
        );
    }
    let bad = b"---\n$type: task\ntitle: A\n---\n";
    assert_eq!(
        code(put_file(&scenario, "/d.md", Expect::Any, bad)),
        Code::RecordBadType
    );
    assert_eq!(file(&scenario, "d.md"), None);
    let twin = b"---\n$type: lr/task/v1\ntitle: A\n---\n";
    assert!(put_file(&scenario, "/d.md", Expect::Any, twin).is_ok());
}

#[test]
fn untyped_frontmatter_that_does_not_parse_is_written_as_a_file() {
    let scenario = board();
    let notes = b"---\ntitle: [draft\n---\n# Draft\n";
    let report = put_file(&scenario, "/n.md", Expect::Any, notes).unwrap();
    assert_eq!(report.writes[0].degraded, None);
    assert_eq!(file(&scenario, "n.md").unwrap(), notes);
}

#[test]
fn a_markdown_record_with_a_dangling_reference_is_written_and_reported_degraded() {
    let scenario = board();
    assert_eq!(
        degraded(
            &scenario,
            "/tasks/a.md",
            "---\n$type: lr/task/v1\ntitle: A\nproject: /projects/q\n---\n"
        ),
        Some(Code::RefDangling)
    );
    assert_eq!(
        degraded(
            &scenario,
            "/tasks/a.md",
            "---\n$type: lr/task/v1\ntitle: A\nproject: /projects/p\n---\n[gone](gone.md)"
        ),
        Some(Code::RefDangling)
    );
    assert_eq!(
        degraded(
            &scenario,
            "/tasks/a.md",
            "---\n$type: lr/task/v1\ntitle: A\nproject: /projects/p\n---\n"
        ),
        None
    );
}

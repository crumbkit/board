//! A repo's file read at its pin, and a write that holds only while the
//! file is as read.

use board_model::Code;

use super::*;
use crate::fake::local;
use crate::refused;

fn own() -> Repo {
    Repo {
        root: "/".to_owned(),
        clone: None,
    }
}

#[test]
fn a_file_is_read_with_its_text_and_blob_and_a_missing_one_is_no_file() {
    let host = local(&[("daily/2026-10-25.md", "# Day\n")]);
    let file = own().read(&host, "notes", "daily/2026-10-25.md").unwrap();
    assert_eq!(file.text.as_deref(), Some("# Day\n"));
    assert!(file.blob.is_some());
    assert!(file.location.is_some());
    let missing = own().read(&host, "notes", "daily/2026-10-26.md").unwrap();
    assert_eq!((missing.text, missing.blob), (None, None));
}

#[test]
fn a_file_that_is_not_utf8_has_a_blob_and_no_text() {
    let host = local(&[]);
    host.scenario
        .write("daily/bytes.md", board_scenario::Item::Bytes(b"\xff\xfe"));
    let file = own().read(&host, "notes", "daily/bytes.md").unwrap();
    assert_eq!(file.text, None);
    assert!(file.blob.is_some());
}

#[test]
fn a_path_that_is_not_a_logical_path_is_refused_as_board_refused() {
    let host = local(&[]);
    let refused = own().read(&host, "notes", "a//b.md").unwrap_err();
    assert_eq!(refused.code, "notes::board::refused");
    assert!(own().read(&host, "notes", "a/b.md").is_ok());
}

#[test]
fn a_write_holds_while_the_file_is_as_read_and_is_stale_once_it_changed() {
    let host = local(&[]);
    own().write(&host, "notes/a.md", "one\n", None).unwrap();
    let read = own().read(&host, "notes", "notes/a.md").unwrap();
    let stale = own().write(&host, "notes/a.md", "two\n", None).unwrap_err();
    assert_eq!(refused(&stale), Code::PutStale);
    own()
        .write(&host, "notes/a.md", "two\n", read.blob.as_deref())
        .unwrap();
    let read = own().read(&host, "notes", "notes/a.md").unwrap();
    assert_eq!(read.text.as_deref(), Some("two\n"));
}

#[test]
fn a_repos_logical_path_joins_its_root_and_the_path() {
    assert_eq!(own().logical("daily/x.md"), "/daily/x.md");
    let mounted = Repo {
        root: "/repos/work".to_owned(),
        clone: Some("/clones/work".to_owned()),
    };
    assert_eq!(mounted.logical("daily/x.md"), "/repos/work/daily/x.md");
}

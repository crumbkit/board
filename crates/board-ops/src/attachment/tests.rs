//! A put with a store uploads large or binary bytes under their digest and
//! writes the record; a fetch checks the digest.

use std::path::Path;

use board_model::{ATTACHMENT_TYPE, Attachment, Code, Digest, Fields, LogicalPath, parse_json};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::put::{Expect, PutError, PutReport};
use crate::tests::open;
use crate::{Backend, Board, OpenError, Store};

/// A board that does not commit, with its store in `store`.
fn stored(store: &Path) -> Scenario {
    let scenario = crate::tests::board();
    let config = json!({
        "$type": "board/config/v1",
        "commit_on_put": false,
        "store": {"backend": "directory", "location": store.display().to_string()},
    });
    scenario.write(".board/board.json", Item::File(&config.to_string()));
    scenario
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn put_file(scenario: &Scenario, at: &str, bytes: &[u8]) -> Result<PutReport, PutError> {
    open(scenario).put_file(&path(at), Expect::Any, bytes)
}

/// The attachment record at `physical` in the working tree.
fn record(scenario: &Scenario, physical: &str) -> Attachment {
    let bytes = std::fs::read(scenario.workdir().join(physical)).unwrap();
    Attachment::from_value(&parse_json(&bytes).unwrap()).unwrap()
}

fn exists(scenario: &Scenario, physical: &str) -> bool {
    scenario.workdir().join(physical).exists()
}

/// Five mebibytes that are not UTF-8.
fn big() -> Vec<u8> {
    (0..5u32 * 1024 * 1024)
        .map(|i| u8::try_from(i % 251).unwrap_or_default())
        .collect()
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test alters or removes what the store holds"
)]
fn tamper(location: &str, bytes: Option<&[u8]>) {
    match bytes {
        Some(bytes) => std::fs::write(location, bytes).unwrap(),
        None => std::fs::remove_file(location).unwrap(),
    }
}

fn fetched(board: &Board, at: &str) -> Result<Vec<u8>, crate::ReadError> {
    let report = board.get(&path(at)).unwrap();
    board.fetched(report).map(|report| report.bytes)
}

#[test]
fn a_large_or_binary_file_put_with_a_store_leaves_only_its_record_in_the_tree_and_its_bytes_under_their_digest()
 {
    let store = tempfile::tempdir().unwrap();
    let scenario = stored(store.path());
    let bytes = big();
    let report = put_file(&scenario, "/files/big.bin", &bytes).unwrap();
    let written = &report.writes[0];
    assert_eq!(written.location.path, "files/big.bin.json");
    assert_eq!(written.display.to_string(), "/files/big.bin");
    assert!(!exists(&scenario, "files/big.bin"));
    let digest = Digest::of(&bytes);
    let location = store.path().join(digest.hex());
    assert_eq!(
        record(&scenario, "files/big.bin.json"),
        Attachment::new(
            digest,
            bytes.len() as u64,
            "application/octet-stream".to_owned(),
            location.display().to_string()
        )
    );
    assert_eq!(std::fs::read(&location).unwrap(), bytes);
    let mut names: Vec<_> = std::fs::read_dir(store.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [digest.hex()]);
    // A small file that is not UTF-8 is attached too, with the media type
    // its name says.
    put_file(&scenario, "/files/icon.png", b"\x89PNG\xff").unwrap();
    assert_eq!(
        record(&scenario, "files/icon.png.json").media_type,
        "image/png"
    );
    // Text up to the inline limit stays in the tree; past it, it is attached.
    put_file(&scenario, "/notes.md", b"# n\n").unwrap();
    assert!(exists(&scenario, "notes.md") && !exists(&scenario, "notes.md.json"));
    // The limit is 1 MiB, as docs/architecture/attachments.md says.
    let limit = vec![b'x'; 1_048_576];
    put_file(&scenario, "/at.md", &limit).unwrap();
    assert!(exists(&scenario, "at.md"));
    put_file(&scenario, "/over.md", &[limit.as_slice(), b"x"].concat()).unwrap();
    assert_eq!(
        record(&scenario, "over.md.json").media_type,
        "text/markdown"
    );
    assert!(!exists(&scenario, "over.md"));
    // check reads the records alone: it passes with the store gone.
    tamper(&location.display().to_string(), None);
    assert!(!open(&scenario).check(None, None).unwrap().degraded());
}

#[test]
fn without_a_store_a_binary_file_is_written_into_the_tree_as_given() {
    let scenario = crate::tests::board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    put_file(&scenario, "/files/icon.png", b"\x89PNG\xff").unwrap();
    assert_eq!(
        std::fs::read(scenario.workdir().join("files/icon.png")).unwrap(),
        b"\x89PNG\xff"
    );
    assert!(!exists(&scenario, "files/icon.png.json"));
}

#[test]
fn a_fetch_returns_bytes_whose_digest_matches_and_a_missing_or_altered_object_is_refused() {
    let store = tempfile::tempdir().unwrap();
    let scenario = stored(store.path());
    let bytes = big();
    put_file(&scenario, "/files/big.bin", &bytes).unwrap();
    let board = open(&scenario);
    let report = board.get(&path("/files/big.bin")).unwrap();
    let whole = board.fetched(report.clone()).unwrap();
    assert_eq!(whole.bytes, bytes);
    assert_eq!(whole.record, report.record);
    assert_eq!(whole.display, report.display);
    // A file that is no attachment passes through as it was read.
    let project = board.get(&path("/projects/p")).unwrap();
    assert_eq!(board.fetched(project.clone()).unwrap(), project);
    let location = store.path().join(Digest::of(&bytes).hex());
    let at = location.display().to_string();
    tamper(&at, Some(b"not the same bytes"));
    let altered = fetched(&board, "/files/big.bin").unwrap_err();
    assert_eq!(altered.code(), Code::AttachmentMismatch);
    assert_eq!(
        altered.to_string(),
        format!(
            "/files/big.bin names {at}, whose bytes hash to {}, not the record's {}",
            Digest::of(b"not the same bytes"),
            Digest::of(&bytes)
        )
    );
    let refusal = altered.refusal("get");
    assert_eq!(refusal.fields, Fields::at(&path("/files/big.bin")).json());
    assert_eq!(
        refusal.hint.as_deref(),
        Some(
            "put the file again with `board put /files/big.bin --file`, which uploads it under its digest and rewrites the record"
        )
    );
    tamper(&at, None);
    let missing = fetched(&board, "/files/big.bin").unwrap_err();
    assert_eq!(missing.code(), Code::AttachmentMissing);
    assert_eq!(
        missing.to_string(),
        format!("/files/big.bin names {at}, which holds nothing")
    );
    // Putting the file again restores the object, and the fetch succeeds.
    put_file(&scenario, "/files/big.bin", &bytes).unwrap();
    assert_eq!(fetched(&board, "/files/big.bin").unwrap(), bytes);
}

#[test]
fn an_attachment_record_that_does_not_read_as_one_is_refused_by_put_check_and_fetch_and_a_whole_one_is_admitted()
 {
    let scenario = stored(Path::new("store"));
    let whole = json!({
        "$type": ATTACHMENT_TYPE,
        "digest": Digest::of(b"x").to_string(),
        "size": 1,
        "media_type": "text/plain",
        "location": "/nowhere/x",
    });
    let board = open(&scenario);
    let put =
        |value: &Value| board.put(&path("/files/x"), Expect::Any, value.to_string().as_bytes());
    assert!(put(&whole).is_ok());
    let mut partial = whole.clone();
    partial.as_object_mut().unwrap().remove("size");
    let refused = put(&partial).unwrap_err();
    assert_eq!(refused.code(), Code::RecordInvalid);
    scenario.write("files/y.json", Item::File(&partial.to_string()));
    let checked = board.check(None, None).unwrap();
    let status = |path: &str| {
        checked
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap()
            .status
    };
    assert_eq!(status("files/x.json"), board_tree::Status::Ok);
    assert_eq!(
        status("files/y.json"),
        board_tree::Status::Degraded(Code::RecordInvalid)
    );
    let unread = fetched(&board, "/files/y").unwrap_err();
    assert_eq!(unread.code(), Code::RecordInvalid);
    assert!(
        unread
            .to_string()
            .starts_with("/files/y is an attachment record that does not read as one: "),
        "{unread}"
    );
    assert_eq!(
        fetched(&board, "/files/x").unwrap_err().code(),
        Code::AttachmentMissing
    );
    // A location that cannot be read, such as a directory, is unreadable.
    let mut directory = whole;
    directory["location"] = json!(scenario.workdir().display().to_string());
    assert!(put(&directory).is_ok());
    let unreadable = fetched(&board, "/files/x").unwrap_err();
    assert_eq!(unreadable.code(), Code::RepoUnreadable);
    assert!(
        unreadable.to_string().contains("which cannot be read: "),
        "{unreadable}"
    );
}

#[test]
fn an_upload_the_store_cannot_take_is_refused_with_put_failed_and_writes_no_record() {
    let blocked = tempfile::tempdir().unwrap();
    let scenario = stored(&blocked.path().join("file/store"));
    scenario.write("files/.keep", Item::File(""));
    tamper(
        &blocked.path().join("file").display().to_string(),
        Some(b""),
    );
    let error = put_file(&scenario, "/files/big.bin", b"\xff").unwrap_err();
    assert_eq!(error.code(), Code::PutFailed);
    assert_eq!(error.fields(), Fields::at(&path("/files/big.bin")));
    assert!(
        error.to_string().starts_with(&format!(
            "/files/big.bin cannot be uploaded to {}: ",
            blocked.path().join("file/store").display()
        )),
        "{error}"
    );
    assert!(!exists(&scenario, "files/big.bin.json"));
    // A relative location is taken from the board's root.
    let scenario = stored(Path::new("store"));
    put_file(&scenario, "/files/big.bin", b"\xff").unwrap();
    let digest = Digest::of(b"\xff");
    assert_eq!(
        record(&scenario, "files/big.bin.json").location,
        scenario
            .workdir()
            .canonicalize()
            .unwrap()
            .join("store")
            .join(digest.hex())
            .display()
            .to_string()
    );
}

#[test]
fn a_store_setting_names_a_backend_board_offers_and_a_location_and_any_other_is_config_invalid() {
    let scenario = stored(Path::new("objects"));
    assert_eq!(
        open(&scenario).config().store,
        Some(Store {
            backend: Backend::Directory,
            location: "objects".to_owned(),
        })
    );
    for invalid in [
        r#"{"backend": "s3", "location": "s3://bucket/prefix"}"#,
        r#"{"backend": "directory"}"#,
        r#"{"backend": "directory", "location": "x", "region": "eu-west-1"}"#,
    ] {
        let config = format!(r#"{{"$type": "board/config/v1", "store": {invalid}}}"#);
        scenario.write(".board/board.json", Item::File(&config));
        let error = Board::open(scenario.workdir(), None, "").unwrap_err();
        assert!(
            matches!(error, OpenError::Config { .. }),
            "{invalid}: {error}"
        );
        assert_eq!(error.code(), Code::ConfigInvalid);
    }
}

#[test]
fn a_plugins_file_put_where_only_the_owner_writes_uploads_nothing_and_its_twin_uploads() {
    let scenario = stored(Path::new("store"));
    let mut board = open(&scenario);
    board.ask_as(crate::Asker {
        plugin: Some("tasks".parse().unwrap()),
        ..crate::Asker::default()
    });
    let bytes = b"\xff\xfe";
    let refused = board.put_file(&path("/effects/no.bin"), Expect::Any, bytes);
    assert_eq!(refused.unwrap_err().code(), Code::SettingsFromPlugin);
    assert!(!exists(&scenario, "store"));
    board
        .put_file(&path("/files/yes.bin"), Expect::Any, bytes)
        .unwrap();
    assert!(exists(
        &scenario,
        &format!("store/{}", Digest::of(bytes).hex())
    ));
}

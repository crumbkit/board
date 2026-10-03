use std::path::Path;
use std::time::Instant;

use board_model::{Code, Kind, ObjectId, Role};
use rusqlite::Connection;

use super::*;

const ROOT: &str = "/boards/one";

fn blob(byte: u8) -> ObjectId {
    ObjectId::Sha1([byte; 20])
}

fn record() -> BlobFact {
    BlobFact {
        kind: Kind::Record,
        record_type: Some("test/task/v1".parse().unwrap()),
        code: None,
    }
}

/// An index on disk holding one blob fact.
fn filled(cache: &Path) -> Index {
    let mut index = Index::open(Some(cache), Path::new(ROOT)).unwrap();
    let writer = index.write().unwrap();
    writer.put_blob(blob(1), Role::Json, &record()).unwrap();
    writer.commit().unwrap();
    index
}

fn has_fact(index: &mut Index) -> bool {
    let writer = index.write().unwrap();
    writer.blob(blob(1), Role::Json).unwrap().is_some()
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test writes garbage where the index lives"
)]
fn overwrite(file: &Path, bytes: &[u8]) {
    std::fs::write(file, bytes).unwrap();
}

#[test]
fn each_board_has_its_own_index_file_under_the_cache() {
    let cache = Path::new("/cache");
    let one = location(cache, Path::new("/boards/one"));
    let two = location(cache, Path::new("/boards/two"));
    assert_ne!(one, two);
    assert!(one.starts_with("/cache/board"));
    assert_eq!(one.file_name().unwrap(), "index.sqlite");
    assert_eq!(
        one.parent().unwrap().file_name().unwrap().len(),
        64,
        "the directory is a SHA-256 in hex"
    );
}

#[test]
fn an_index_reopens_with_the_facts_it_was_given() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(
        index.storage(),
        &Storage::File(location(cache.path(), Path::new(ROOT)))
    );
    let writer = index.write().unwrap();
    assert_eq!(writer.blob(blob(1), Role::Json).unwrap(), Some(record()));
    assert_eq!(writer.blob(blob(1), Role::Schema).unwrap(), None);
}

#[test]
fn a_corrupt_index_is_deleted_and_rebuilt() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let file = location(cache.path(), Path::new(ROOT));
    overwrite(
        &file,
        b"this is not a database, and it is long enough to be read",
    );
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::File(file));
    assert!(!has_fact(&mut index));
}

#[test]
fn an_index_of_another_format_is_deleted_and_rebuilt() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let file = location(cache.path(), Path::new(ROOT));
    Connection::open(&file)
        .unwrap()
        .execute(
            "UPDATE state SET value = '0' WHERE key = 'index_version'",
            [],
        )
        .unwrap();
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::File(file));
    assert!(!has_fact(&mut index));
}

#[test]
fn an_index_of_this_format_is_kept() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let file = location(cache.path(), Path::new(ROOT));
    Connection::open(&file)
        .unwrap()
        .execute(
            "UPDATE state SET value = ?1 WHERE key = 'index_version'",
            [INDEX_VERSION],
        )
        .unwrap();
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(has_fact(&mut index));
}

#[test]
fn an_index_another_process_holds_past_the_wait_is_replaced_by_one_in_memory() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let holder = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let started = Instant::now();
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(
        started.elapsed().as_secs() >= 5,
        "it waits before giving up"
    );
    assert_eq!(index.storage(), &Storage::Memory(Fallback::Busy));
    assert!(!has_fact(&mut index), "the index in memory starts empty");
}

#[test]
fn an_index_another_process_has_released_is_used() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let holder = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE; COMMIT").unwrap();
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(matches!(index.storage(), Storage::File(_)));
    assert!(has_fact(&mut index));
}

#[test]
fn with_no_cache_the_index_is_in_memory() {
    let index = Index::open(None, Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::Memory(Fallback::NoCache));
}

#[test]
fn a_validation_stores_its_outcome_and_the_references_it_found_in_order() {
    let cache = tempfile::tempdir().unwrap();
    let mut index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = index.write().unwrap();
    let valid = ValidationFact::Valid(vec![
        RefFact {
            pointer: "/owner".to_owned(),
            target: "/people/ada".parse().unwrap(),
            accepts: Some(vec!["test/person/v1".parse().unwrap()]),
        },
        RefFact {
            pointer: "/blocks".to_owned(),
            target: "/tasks/b".parse().unwrap(),
            accepts: None,
        },
    ]);
    let invalid = ValidationFact::Invalid {
        code: Code::RecordInvalid,
        pointer: "/due".to_owned(),
    };
    assert_eq!(writer.validation(blob(1), blob(2)).unwrap(), None);
    writer.put_validation(blob(1), blob(2), &valid).unwrap();
    writer.put_validation(blob(3), blob(2), &invalid).unwrap();
    assert_eq!(writer.validation(blob(1), blob(2)).unwrap(), Some(valid));
    assert_eq!(writer.validation(blob(3), blob(2)).unwrap(), Some(invalid));
    assert_eq!(writer.validation(blob(1), blob(4)).unwrap(), None);
    writer.commit().unwrap();
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let rows: Vec<(String, Option<String>, Option<String>)> = file
        .prepare("SELECT record, code, pointer FROM validations ORDER BY record")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [
            (blob(1).to_string(), None, None),
            (
                blob(3).to_string(),
                Some("board::record::invalid".to_owned()),
                Some("/due".to_owned())
            ),
        ]
    );
    let accepts: String = file
        .query_row("SELECT accepts FROM refs WHERE ordinal = 0", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(accepts, r#"["test/person/v1"]"#, "agents read it as JSON");
}

#[test]
fn a_query_cannot_write_and_leaves_the_index_writable() {
    let cache = tempfile::tempdir().unwrap();
    let mut index = filled(cache.path());
    let refused = index.query("DELETE FROM blobs", 10).unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    let admitted = index.query("SELECT count(*) AS n FROM blobs", 10).unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(1)]]);
    assert!(
        has_fact(&mut index),
        "the write lock is still there to take"
    );
    let writer = index.write().unwrap();
    writer.put_blob(blob(2), Role::Json, &record()).unwrap();
    writer.commit().unwrap();
}

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use board_model::{Code, Kind, ObjectId, Role, Source, canonical_json};
use board_tree::MountPoint;
use rusqlite::Connection;

use super::*;
use crate::store::{MountRow, SourceRow};

mod cache;
mod prune;
mod worktree;

const ROOT: &str = "/boards/one";

fn blob(byte: u8) -> ObjectId {
    ObjectId::Sha1([byte; 20])
}

fn record() -> BlobFact {
    BlobFact {
        kind: Kind::Record,
        record_type: Some("test/task/v1".parse().unwrap()),
        code: None,
        reason: None,
    }
}

/// A file that does not parse, with the parser's message.
fn unparsed() -> BlobFact {
    BlobFact {
        kind: Kind::Json,
        record_type: None,
        code: Some(Code::JsonParse),
        reason: Some("json does not parse: EOF at line 1 column 1".to_owned()),
    }
}

/// The view of an index on disk holding one blob fact.
fn filled(cache: &Path) -> View {
    let writer = Index::open(Some(cache), Path::new(ROOT))
        .unwrap()
        .write()
        .unwrap();
    writer.put_blob(blob(1), Role::Json, &record()).unwrap();
    writer.decide(Vec::new()).unwrap().view().unwrap()
}

/// `index` once a writer has decided no entries.
fn decided(index: Index) -> View {
    index
        .write()
        .unwrap()
        .decide(Vec::new())
        .unwrap()
        .view()
        .unwrap()
}

fn has_fact(index: Index) -> bool {
    let writer = index.deferred().unwrap();
    writer.blob(blob(1), Role::Json).unwrap().is_some()
}

#[test]
fn a_writer_counts_each_blob_lookup_whether_or_not_the_index_holds_the_blob() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    writer.put_blob(blob(1), Role::Json, &record()).unwrap();
    assert_eq!(writer.blobs_read(), 0);
    assert!(writer.blob(blob(1), Role::Json).unwrap().is_some());
    assert!(writer.blob(blob(2), Role::Json).unwrap().is_none());
    assert_eq!(writer.blobs_read(), 2);
}

/// Writes one blob fact to a new index file, then decides when `decided`
/// or drops the writer otherwise. Returns whether the file then holds it.
fn kept_after(decided: bool) -> bool {
    let cache = tempfile::tempdir().unwrap();
    let open = || Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = open().write().unwrap();
    writer.put_blob(blob(1), Role::Json, &record()).unwrap();
    if decided {
        drop(writer.decide(Vec::new()).unwrap());
    } else {
        drop(writer);
    }
    has_fact(open())
}

#[test]
fn a_writer_dropped_before_it_decides_rolls_back_what_it_wrote() {
    assert!(!kept_after(false));
}

#[test]
fn a_writer_that_decides_commits_what_it_wrote() {
    assert!(kept_after(true));
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
    assert_eq!(
        one.file_name().unwrap().to_str().unwrap(),
        format!("index-{INDEX_VERSION}.sqlite"),
        "the name carries the format"
    );
    assert_eq!(
        one.parent().unwrap().file_name().unwrap().len(),
        64,
        "the directory is a SHA-256 in hex"
    );
}

#[test]
fn a_blob_that_fails_keeps_its_reason_and_one_that_parses_has_none() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    writer.put_blob(blob(1), Role::Json, &record()).unwrap();
    writer.put_blob(blob(7), Role::Json, &unparsed()).unwrap();
    assert_eq!(writer.blob(blob(7), Role::Json).unwrap(), Some(unparsed()));
    assert_eq!(writer.blob(blob(1), Role::Json).unwrap(), Some(record()));
}

#[test]
fn an_index_reopens_with_the_facts_it_was_given() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(
        index.storage(),
        &Storage::File(location(cache.path(), Path::new(ROOT)))
    );
    let writer = index.write().unwrap();
    assert_eq!(writer.blob(blob(1), Role::Json).unwrap(), Some(record()));
    assert_eq!(writer.blob(blob(1), Role::Schema).unwrap(), None);
}

#[test]
fn an_index_that_falls_back_is_empty_and_in_memory_and_leaves_the_file() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let index = Index::fall_back().unwrap();
    assert_eq!(index.storage(), &Storage::Memory(Fallback::Busy));
    assert!(!has_fact(index));
    let file = location(cache.path(), Path::new(ROOT));
    assert_eq!(holds(&file), (1, INDEX_VERSION.to_string()));
}

#[test]
fn a_new_index_file_has_16_kib_pages_and_keeps_each_body_once_in_a_rowid_table() {
    let cache = tempfile::tempdir().unwrap();
    let writer = Index::open(Some(cache.path()), Path::new(ROOT))
        .unwrap()
        .write()
        .unwrap();
    writer
        .put_body(blob(1), &canonical_json(&serde_json::json!({"a": 1})))
        .unwrap();
    writer
        .put_body(blob(1), &canonical_json(&serde_json::json!({"a": 1})))
        .unwrap();
    drop(writer.decide(Vec::new()).unwrap());
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let page_size: i64 = file
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
    assert_eq!(page_size, 16_384);
    let rows: Vec<(i64, String)> = file
        .prepare("SELECT rowid, blob FROM bodies")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, [(1, blob(1).to_string())]);
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
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::File(file));
    assert!(!has_fact(index));
}

#[test]
fn an_index_whose_format_differs_from_its_name_is_deleted_and_rebuilt() {
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
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::File(file));
    assert!(!has_fact(index));
}

/// Leaves the filled index's fact only in a file called `name` beside it,
/// of format `version`, and returns a connection holding that file open,
/// as the build that uses it would.
fn beside(cache: &Path, name: &str, version: &str) -> (PathBuf, Connection) {
    drop(filled(cache));
    let file = location(cache, Path::new(ROOT));
    let other = file.with_file_name(name);
    if other != file {
        let connection = Connection::open(&file).unwrap();
        connection
            .execute("VACUUM INTO ?1", [other.to_str().unwrap()])
            .unwrap();
        connection.execute("DELETE FROM blobs", []).unwrap();
    }
    let holder = Connection::open(&other).unwrap();
    holder
        .execute(
            "UPDATE state SET value = ?1 WHERE key = 'index_version'",
            [version],
        )
        .unwrap();
    (other, holder)
}

/// The facts and format of the file at `path`, read afresh.
fn holds(path: &Path) -> (i64, String) {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT (SELECT count(*) FROM blobs),
                    (SELECT value FROM state WHERE key = 'index_version')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn an_index_another_format_uses_is_left_to_the_build_that_has_it_open() {
    let cache = tempfile::tempdir().unwrap();
    // Builds before the name carried the format used `index.sqlite`, and
    // format 8 is one of theirs.
    let (other, _holder) = beside(cache.path(), "index.sqlite", "8");
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(
        index.storage(),
        &Storage::File(location(cache.path(), Path::new(ROOT)))
    );
    assert!(!has_fact(index), "it opened its own file");
    assert_eq!(holds(&other), (1, "8".to_owned()), "the other is intact");
}

#[test]
fn an_index_of_this_format_is_reused_while_another_build_has_it_open() {
    let cache = tempfile::tempdir().unwrap();
    let file = location(cache.path(), Path::new(ROOT));
    let name = file.file_name().unwrap().to_str().unwrap();
    let (other, _holder) = beside(cache.path(), name, &INDEX_VERSION.to_string());
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::File(file));
    assert!(has_fact(index), "it opened the same file");
    assert_eq!(holds(&other), (1, INDEX_VERSION.to_string()));
}

#[test]
fn an_index_another_process_holds_past_the_wait_is_replaced_by_one_in_memory() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let holder = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    holder
        .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE")
        .unwrap();
    let started = Instant::now();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(
        started.elapsed().as_secs() >= 5,
        "it waits before giving up"
    );
    assert_eq!(index.storage(), &Storage::Memory(Fallback::Busy));
    assert!(!has_fact(index), "the index in memory starts empty");
}

#[test]
fn an_index_another_process_is_writing_is_used_without_waiting_for_it() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let holder = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(
        matches!(index.storage(), Storage::File(_)),
        "a reader shares the file with one writer"
    );
    assert!(has_fact(index));
}

#[test]
fn an_index_another_process_has_released_is_used() {
    let cache = tempfile::tempdir().unwrap();
    drop(filled(cache.path()));
    let holder = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE; COMMIT").unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    assert!(matches!(index.storage(), Storage::File(_)));
    assert!(has_fact(index));
}

#[test]
fn with_no_cache_the_index_is_in_memory() {
    let index = Index::open(None, Path::new(ROOT)).unwrap();
    assert_eq!(index.storage(), &Storage::Memory(Fallback::NoCache));
}

/// One source and one mount, the mount resolved to `target` or failed.
fn mounting(target: Result<ObjectId, Code>) -> (Source, Vec<MountPoint>) {
    let source = Source::Commit(blob(2));
    let mount = MountPoint {
        source: source.clone(),
        path: "vendor/other".into(),
        target,
    };
    (source, vec![mount])
}

/// Stores the sources and mounts `first`, then sets `then` in a deferred
/// transaction. Returns whether it wrote, and what the index then holds.
fn remount(
    first: Result<ObjectId, Code>,
    then: Result<ObjectId, Code>,
) -> (bool, (BTreeSet<SourceRow>, BTreeSet<MountRow>)) {
    let index = Index::open(None, Path::new(ROOT)).unwrap();
    let repository = Path::new(ROOT);
    let (source, mounts) = mounting(first);
    let writer = index.write().unwrap();
    writer
        .set_mounts(&[(source, Some(blob(3)), repository)], &mounts)
        .unwrap();
    let index = writer.decide(Vec::new()).unwrap().index;
    let (source, mounts) = mounting(then);
    let writer = index.deferred().unwrap();
    writer
        .set_mounts(&[(source, Some(blob(3)), repository)], &mounts)
        .unwrap();
    (writer.wrote().unwrap(), writer.mounted().unwrap())
}

#[test]
fn setting_the_sources_and_mounts_the_index_holds_writes_nothing() {
    let (wrote, (sources, mounts)) = remount(Ok(blob(4)), Ok(blob(4)));
    assert!(!wrote);
    assert_eq!(sources.len(), 1);
    assert_eq!(
        mounts
            .into_iter()
            .map(|row| (row.2, row.3))
            .collect::<Vec<_>>(),
        [(Some(blob(4).to_string()), None)]
    );
}

#[test]
fn setting_other_sources_or_mounts_writes_them() {
    let (wrote, (sources, mounts)) = remount(Ok(blob(4)), Err(Code::MountInvalid));
    assert!(wrote);
    assert_eq!(sources.len(), 1);
    assert_eq!(
        mounts
            .into_iter()
            .map(|row| (row.2, row.3))
            .collect::<Vec<_>>(),
        [(None, Some(Code::MountInvalid.as_str().to_owned()))]
    );
}

#[test]
fn a_validation_stores_its_outcome_and_the_references_it_found_in_order() {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    );
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
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
        reason: "5 is not of type \"string\"".to_owned(),
        failures: 2,
    };
    assert_eq!(writer.validation(blob(1), blob(2)).unwrap(), None);
    writer.put_validation(blob(1), blob(2), &valid).unwrap();
    writer.put_validation(blob(3), blob(2), &invalid).unwrap();
    assert_eq!(writer.validation(blob(1), blob(2)).unwrap(), Some(valid));
    assert_eq!(writer.validation(blob(3), blob(2)).unwrap(), Some(invalid));
    assert_eq!(writer.validation(blob(1), blob(4)).unwrap(), None);
    writer.decide(Vec::new()).unwrap();
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let rows: Vec<Row> = file
        .prepare("SELECT record, code, pointer, reason, failures FROM validations ORDER BY record")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [
            (blob(1).to_string(), None, None, None, None),
            (
                blob(3).to_string(),
                Some("board::record::invalid".to_owned()),
                Some("/due".to_owned()),
                Some("5 is not of type \"string\"".to_owned()),
                Some(2)
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
    let view = filled(cache.path());
    let refused = view.query("DELETE FROM main.blobs", 10, None).unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    let admitted = view
        .query("SELECT count(*) AS n FROM main.blobs", 10, None)
        .unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(1)]]);
    let writer = view.index.write().unwrap();
    writer.put_blob(blob(2), Role::Json, &record()).unwrap();
    writer.decide(Vec::new()).unwrap();
}

#[test]
fn a_query_keeps_the_rows_within_its_budget_and_always_the_first() {
    let cache = tempfile::tempdir().unwrap();
    let view = filled(cache.path());
    let kept = |bytes| {
        let budget = crate::Budget { bytes, size: |_| 1 };
        let rows = view
            .query("SELECT * FROM (VALUES (1), (2), (3))", 10, Some(budget))
            .unwrap();
        (rows.rows.len(), rows.truncated, rows.over_budget)
    };
    assert_eq!(kept(3), (3, false, false), "3 bytes fit 3 rows of 1");
    assert_eq!(kept(2), (2, true, true), "2 bytes fit 2");
    assert_eq!(kept(0), (1, true, true), "the first row is kept");
}

#[test]
fn a_failure_of_the_database_is_the_indexs_and_any_other_is_the_querys() {
    let failure = |code| rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);
    for code in [
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_LOCKED,
        rusqlite::ffi::SQLITE_IOERR,
        rusqlite::ffi::SQLITE_CORRUPT,
        rusqlite::ffi::SQLITE_NOTADB,
        rusqlite::ffi::SQLITE_NOMEM,
        rusqlite::ffi::SQLITE_CANTOPEN,
    ] {
        assert_eq!(
            crate::query::blame(failure(code)).code(),
            Code::IndexFailed,
            "{code}"
        );
    }
    for code in [rusqlite::ffi::SQLITE_ERROR, rusqlite::ffi::SQLITE_READONLY] {
        assert_eq!(
            crate::query::blame(failure(code)).code(),
            Code::QueryInvalid,
            "{code}"
        );
    }
    assert_eq!(
        crate::query::blame(rusqlite::Error::MultipleStatement).code(),
        Code::QueryInvalid
    );
}

fn journal_mode(view: &View) -> String {
    view.index
        .connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_query_cannot_run_a_pragma_but_can_call_a_pragma_function() {
    let cache = tempfile::tempdir().unwrap();
    let index = filled(cache.path());
    let refused = index
        .query("PRAGMA journal_mode=delete", 10, None)
        .unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    assert_eq!(journal_mode(&index), "wal");
    let admitted = index
        .query(
            "SELECT name FROM pragma_table_info('blobs') LIMIT 1",
            10,
            None,
        )
        .unwrap();
    assert_eq!(admitted.rows.len(), 1);
    assert_eq!(journal_mode(&index), "wal");
}

#[test]
fn a_query_cannot_attach_a_database_but_a_plugin_database_attaches() {
    let cache = tempfile::tempdir().unwrap();
    let index = filled(cache.path());
    let target = cache.path().join("x.sqlite");
    let sql = format!("ATTACH '{}' AS x", target.display());
    let refused = index.query(&sql, 10, None).unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    assert!(!target.exists(), "no file is created");
    let refused = index.query("DETACH DATABASE temp", 10, None).unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    let plugin = cache.path().join("plugin.sqlite");
    rusqlite::Connection::open(&plugin)
        .unwrap()
        .execute_batch("CREATE TABLE notes (n INTEGER); INSERT INTO notes VALUES (7)")
        .unwrap();
    index.attach("p_notes", &plugin).unwrap();
    let admitted = index
        .query("SELECT n FROM p_notes.notes", 10, None)
        .unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(7)]]);
}

#[test]
fn a_query_that_runs_too_long_is_stopped_and_a_bounded_one_finishes() {
    let cache = tempfile::tempdir().unwrap();
    let index = filled(cache.path());
    let unbounded = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c) \
                     SELECT count(*) FROM c";
    let refused = index.query(unbounded, 10, None).unwrap_err();
    assert_eq!(refused.code(), Code::QueryTimeout);
    let bounded = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c WHERE n < 10) \
                   SELECT count(*) FROM c";
    let admitted = index.query(bounded, 10, None).unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(10)]]);
    assert_eq!(journal_mode(&index), "wal");
    assert!(has_fact(index.index), "the index is still writable");
}

#[test]
fn a_query_cannot_vacuum_into_a_file() {
    let cache = tempfile::tempdir().unwrap();
    let index = filled(cache.path());
    let target = cache.path().join("v.sqlite");
    let sql = format!("VACUUM INTO '{}'", target.display());
    let refused = index.query(&sql, 10, None).unwrap_err();
    assert_eq!(refused.code(), Code::QueryInvalid);
    assert!(!target.exists(), "no file is created");
    let admitted = index
        .query("SELECT count(*) AS n FROM main.blobs", 10, None)
        .unwrap();
    assert_eq!(admitted.rows, [vec![serde_json::json!(1)]]);
}

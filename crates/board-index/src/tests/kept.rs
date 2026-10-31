//! Kept decisions read back: by path, with their detail, and refused with
//! the index code when a value cannot be read.

use super::*;

/// One mounted entry, as a source's decision keeps it.
fn row(path: &str, code: Option<Code>) -> Resolved {
    Resolved {
        source: Source::Commit(blob(9)),
        path: path.to_owned(),
        logical: Some(format!("/vendor/{path}").parse().unwrap()),
        blob: Some(blob(1)),
        size: Some(2),
        role: Some(Role::Json),
        schema: None,
        kind: Kind::Record,
        code,
        change: board_model::Change::Committed,
    }
}

/// The rows kept under one key after its one row's `column` is set to
/// `value` in the file, or the code and message reading them fails with.
fn kept_with(column: &str, value: &str) -> Result<Vec<KeptRow>, (Code, String)> {
    let cache = tempfile::tempdir().unwrap();
    let key = board_model::Digest::of(b"key");
    let open = || Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = open().write().unwrap();
    let entry = row("tasks/a", Some(Code::RefDangling));
    writer.put_decision(&key, [(&entry, None)]).unwrap();
    writer.decide(Vec::new()).unwrap();
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let sql = format!("UPDATE decided SET {column} = ?1");
    file.execute(&sql, [value]).unwrap();
    drop(file);
    let writer = open().write().unwrap();
    (writer.decision(&key)).map_err(|error| (error.code(), error.to_string()))
}

#[test]
fn a_kept_row_whose_logical_path_kind_or_code_cannot_be_read_fails_with_the_index_code_and_its_twin_reads()
 {
    let named = "/vendor/tasks/a (tasks/a in its source)";
    for (column, admitted, refused, path) in [
        ("logical", "/vendor/tasks/a", "no/slash", "tasks/a"),
        ("kind", "record", "recorded", named),
        ("code", "board::ref::dangling", "board::ref::lost", named),
    ] {
        let rows = kept_with(column, admitted).unwrap();
        let expected = KeptRow {
            path: "tasks/a".to_owned(),
            logical: Some("/vendor/tasks/a".parse().unwrap()),
            kind: Kind::Record,
            code: Some(Code::RefDangling),
            detail: None,
        };
        assert_eq!(rows, [expected], "{column}");
        assert_eq!(
            kept_with(column, refused),
            Err((
                Code::IndexFailed,
                format!("the index keeps {column} {refused:?} for {path}, which cannot be read")
            )),
            "{column}"
        );
    }
}

#[test]
fn kept_rows_come_back_by_path_with_their_detail() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = board_model::Digest::of(b"key");
    let (b, a) = (row("b", None), row("a", Some(Code::JsonParse)));
    writer
        .put_decision(&key, [(&b, None), (&a, Some("{}".to_owned()))])
        .unwrap();
    let rows = writer.decision(&key).unwrap();
    let read: Vec<(&str, Option<Code>, Option<&str>)> = (rows.iter())
        .map(|row| (row.path.as_str(), row.code, row.detail.as_deref()))
        .collect();
    assert_eq!(
        read,
        [("a", Some(Code::JsonParse), Some("{}")), ("b", None, None)]
    );
}

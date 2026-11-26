//! Kept decisions and their references read back: by path, with their
//! detail, and refused with the index code when a value cannot be read.

use board_model::{Location, Object};

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
fn a_kept_row_whose_logical_path_kind_code_or_change_cannot_be_read_fails_with_the_index_code_and_its_twin_reads()
 {
    let named = "/vendor/tasks/a (tasks/a in its source)";
    let hex = "01".repeat(20);
    for (column, admitted, refused, path) in [
        ("logical", "/vendor/tasks/a", "no/slash", "tasks/a"),
        ("blob", hex.as_str(), "nothex", named),
        ("kind", "record", "recorded", named),
        ("code", "board::ref::dangling", "board::ref::lost", named),
        ("change", "uncommitted", "changed", named),
    ] {
        let rows = kept_with(column, admitted).unwrap();
        let expected = KeptRow {
            path: "tasks/a".to_owned(),
            logical: Some("/vendor/tasks/a".parse().unwrap()),
            blob: Some(blob(1)),
            kind: Kind::Record,
            code: Some(Code::RefDangling),
            detail: None,
            change: if column == "change" {
                board_model::Change::Uncommitted
            } else {
                board_model::Change::Committed
            },
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

#[test]
fn a_working_tree_row_keeps_its_change_and_a_mounted_row_reads_as_committed() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = board_model::Digest::of(b"key");
    let uncommitted = |source| Resolved {
        source,
        change: board_model::Change::Uncommitted,
        ..row("a", None)
    };
    let (worktree, mounted) = (
        uncommitted(Source::WorkingTree),
        uncommitted(Source::Commit(blob(9))),
    );
    for (entry, change) in [
        (worktree, board_model::Change::Uncommitted),
        (mounted, board_model::Change::Committed),
    ] {
        writer.put_decision(&key, [(&entry, None)]).unwrap();
        assert_eq!(
            writer.decision(&key).unwrap()[0].change,
            change,
            "{:?}",
            entry.source
        );
    }
}

/// A location in the board at `repository`, read from `source`.
fn at(repository: &str, source: Source, path: &str, object: Object) -> Location {
    Location {
        board: board_model::Board {
            repository: repository.into(),
            source,
        },
        path: path.to_owned(),
        object,
        pointers: Vec::new(),
        follow: None,
    }
}

/// The references one mounted entry, `tasks/a`, and one Markdown file,
/// `notes.md`, make, the entry's in its source `own`: one within the
/// source through a link, one through a mount into another commit, one
/// into a directory, one that dangles, and a link from the file.
fn references(own: &Source) -> Vec<(&'static str, Reached)> {
    let reached = |pointer: &str, location, code| Reached {
        pointer: pointer.to_owned(),
        link: false,
        target: "/projects/p".parse().unwrap(),
        location,
        code,
    };
    let mut linked = at(
        "/boards/lib",
        own.clone(),
        "projects/p.json",
        Object::Blob(blob(3)),
    );
    linked.pointers.push(at(
        "/boards/lib",
        own.clone(),
        "links/l.json",
        Object::Blob(blob(4)),
    ));
    let mut mounted = at(
        "/boards/two",
        Source::Commit(blob(7)),
        "tasks/b.json",
        Object::Blob(blob(5)),
    );
    mounted.pointers.push(Location {
        follow: Some("main".to_owned()),
        ..at(
            "/boards/lib",
            own.clone(),
            "vendor.json",
            Object::Blob(blob(6)),
        )
    });
    let directory = at("/boards/lib", own.clone(), "projects", Object::Directory);
    vec![
        ("tasks/a", reached("/project", Some(linked), None)),
        ("tasks/a", reached("/see", Some(mounted), None)),
        (
            "tasks/a",
            reached("/dir", Some(directory), Some(Code::RefWrongType)),
        ),
        ("tasks/a", reached("/none", None, Some(Code::RefDangling))),
        (
            "notes.md",
            Reached {
                link: true,
                ..reached("../x.md", None, Some(Code::RefDangling))
            },
        ),
    ]
}

/// `references` kept under `key` for the source `own`, with a kept row for
/// `tasks/a` and none for `notes.md`.
fn kept_references(writer: &Writer, key: &board_model::Digest, own: &Source) {
    let entry = row("tasks/a", None);
    writer.put_decision(key, [(&entry, None)]).unwrap();
    let references = references(own);
    let each = references.iter().map(|(path, reached)| (*path, reached));
    writer.put_references(key, own, each).unwrap();
}

#[test]
fn kept_references_come_back_by_path_in_order_with_where_they_led() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = board_model::Digest::of(b"key");
    let own = Source::Commit(blob(9));
    kept_references(&writer, &key, &own);
    let mut expected: Vec<KeptReference> = (references(&own).into_iter())
        .map(|(path, reached)| {
            let logical = (path == "tasks/a").then(|| "/vendor/tasks/a".parse().unwrap());
            (path.to_owned(), logical, reached)
        })
        .collect();
    expected.rotate_right(1);
    assert_eq!(writer.references(&key, &own).unwrap(), expected);
}

#[test]
fn a_kept_reference_into_its_own_source_names_the_source_it_is_read_for_and_one_elsewhere_keeps_its_own()
 {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = board_model::Digest::of(b"key");
    kept_references(&writer, &key, &Source::Commit(blob(9)));
    // A commit of the same tree reads the same decision.
    let other = Source::Commit(blob(8));
    let read = writer.references(&key, &other).unwrap();
    let led: Vec<Vec<Source>> = (read.iter())
        .map(|(_, _, reached)| {
            let location = reached.location.iter();
            (location.flat_map(|at| at.pointers.iter().chain([at])))
                .map(|at| at.board.source.clone())
                .collect()
        })
        .collect();
    assert_eq!(
        led,
        [
            vec![],
            vec![other.clone(), other.clone()],
            vec![other.clone(), Source::Commit(blob(7))],
            vec![other],
            vec![],
        ]
    );
}

/// The references kept under one key after one reference's `column` is set
/// to `value` in the file, or the code and message reading them fails with.
fn references_with(column: &str, value: &str) -> Result<Vec<KeptReference>, (Code, String)> {
    let cache = tempfile::tempdir().unwrap();
    let key = board_model::Digest::of(b"key");
    let own = Source::Commit(blob(9));
    let open = || Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = open().write().unwrap();
    kept_references(&writer, &key, &own);
    writer.decide(Vec::new()).unwrap();
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let sql =
        format!("UPDATE resolutions SET {column} = ?1 WHERE path = 'tasks/a' AND ordinal = 0");
    file.execute(&sql, [value]).unwrap();
    drop(file);
    let writer = open().write().unwrap();
    (writer.references(&key, &own)).map_err(|error| (error.code(), error.to_string()))
}

#[test]
fn a_kept_reference_whose_target_location_or_code_cannot_be_read_fails_with_the_index_code_and_its_twin_reads()
 {
    let named = "/vendor/tasks/a (tasks/a in its source)";
    let directory = r#"{"repository": "/b", "source": null, "path": "", "object": null, "pointers": [], "follow": null}"#;
    for (column, admitted, refused) in [
        ("target", "/projects/q", "no/slash"),
        ("location", directory, r#"{"repository": "/b"}"#),
        ("location", directory, "not json"),
        ("code", "board::ref::dangling", "board::ref::lost"),
    ] {
        let read = references_with(column, admitted).unwrap();
        assert_eq!(read.len(), 5, "{column}");
        assert_eq!(
            references_with(column, refused),
            Err((
                Code::IndexFailed,
                format!("the index keeps {column} {refused:?} for {named}, which cannot be read")
            )),
            "{column}"
        );
    }
}

#[test]
fn a_prune_deletes_the_references_of_a_decision_no_source_reached_and_keeps_those_of_one_it_did() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let [reached, gone] = [b"reached".as_slice(), b"gone"].map(board_model::Digest::of);
    let own = Source::Commit(blob(9));
    for key in [&reached, &gone] {
        kept_references(&writer, key, &own);
    }
    writer.reach(&own, Some(&reached), None).unwrap();
    writer.prune().unwrap();
    assert_eq!(writer.references(&reached, &own).unwrap().len(), 5);
    assert_eq!(writer.references(&gone, &own).unwrap(), Vec::new());
}

#[test]
fn forgetting_paths_of_a_slot_deletes_their_references_and_keeps_the_others() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let slot = board_model::Digest::of(b"slot");
    let own = Source::WorkingTree;
    kept_references(&writer, &slot, &own);
    writer.forget(&slot, Some(&["tasks/a"])).unwrap();
    let paths: Vec<String> = (writer.references(&slot, &own).unwrap().into_iter())
        .map(|(path, _, _)| path)
        .collect();
    assert_eq!(paths, ["notes.md"]);
    writer.forget(&slot, None).unwrap();
    assert_eq!(writer.references(&slot, &own).unwrap(), Vec::new());
}

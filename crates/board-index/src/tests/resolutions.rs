//! Kept references read back: by path, each path's in order, with where
//! each one led, and refused with the index code when one cannot be read.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;

use board_model::{Board, Digest, Location, Object};

use super::*;

/// A location in the board at `repository`, read from `source`.
fn at(repository: &Path, source: Source, path: &str, object: Object) -> Location {
    Location {
        board: Board {
            repository: repository.to_path_buf(),
            source,
        },
        path: path.to_owned(),
        object,
        pointers: Vec::new(),
        follow: None,
    }
}

/// The mounted board `/boards/lib` read from `source`.
fn lib(source: Source) -> Board {
    Board {
        repository: "/boards/lib".into(),
        source,
    }
}

/// The references `tasks/a` and `notes.md` make in the board `own`: one
/// within it through a link, one through a mount into another commit, one
/// into a directory, one into the same commit read from another clone, one
/// into a clone whose path is not UTF-8, one that dangles, and a link from
/// the Markdown file.
fn references(own: &Board) -> Vec<(&'static str, Reached)> {
    let reached = |pointer: &str, location, code| Reached {
        pointer: pointer.to_owned(),
        link: false,
        target: "/projects/p".parse().unwrap(),
        location,
        code,
    };
    let source = || own.source.clone();
    let here = |path: &str, object| at(&own.repository, source(), path, object);
    let mut linked = here("projects/p.json", Object::Blob(blob(3)));
    linked
        .pointers
        .push(here("links/l.json", Object::Blob(blob(4))));
    let two = Path::new("/boards/two");
    let mut mounted = at(
        two,
        Source::Commit(blob(7)),
        "tasks/b.json",
        Object::Blob(blob(5)),
    );
    mounted.pointers.push(Location {
        follow: Some("main".to_owned()),
        ..here("vendor.json", Object::Blob(blob(6)))
    });
    let clone = at(
        Path::new("/boards/clone"),
        source(),
        "c.json",
        Object::Blob(blob(2)),
    );
    let bytes = Path::new(OsStr::from_bytes(b"/boards/\xff"));
    let unnamed = at(bytes, source(), "d.json", Object::Blob(blob(2)));
    let directory = here("projects", Object::Directory);
    vec![
        ("tasks/a", reached("/project", Some(linked), None)),
        ("tasks/a", reached("/see", Some(mounted), None)),
        (
            "tasks/a",
            reached("/dir", Some(directory), Some(Code::RefWrongType)),
        ),
        ("tasks/a", reached("/clone", Some(clone), None)),
        ("tasks/a", reached("/bytes", Some(unnamed), None)),
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

/// `references` kept under `key` for the board `own`.
fn kept(writer: &Writer, key: &Digest, own: &Board) {
    let references = references(own);
    let each = references.iter().map(|(path, reached)| (*path, reached));
    writer.put_references(key, own, each).unwrap();
}

/// What `references` gives, as reading them back groups them.
fn grouped(own: &Board) -> Vec<KeptReferences> {
    let mut grouped: Vec<KeptReferences> = Vec::new();
    for (path, reached) in references(own) {
        match grouped.iter_mut().find(|(was, _)| was == path) {
            Some((_, each)) => each.push(reached),
            None => grouped.push((path.to_owned(), vec![reached])),
        }
    }
    grouped.sort_by(|a, b| a.0.cmp(&b.0));
    grouped
}

/// How many rows the index keeps under `key`.
fn rows(writer: &Writer, key: &Digest) -> i64 {
    let sql = "SELECT count(*) FROM main.resolutions WHERE key = ?1";
    (writer
        .connection()
        .query_row(sql, [key.hex()], |row| row.get(0)))
    .unwrap()
}

#[test]
fn kept_references_come_back_by_path_each_in_order_with_where_they_led_in_one_row_per_path() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = Digest::of(b"key");
    let own = lib(Source::Commit(blob(9)));
    kept(&writer, &key, &own);
    assert_eq!(writer.references(&key, &own).unwrap(), grouped(&own));
    assert_eq!(rows(&writer, &key), 2, "one row for each path");
}

#[test]
fn a_kept_reference_into_its_own_board_names_the_board_it_is_read_for_and_one_elsewhere_keeps_its_own()
 {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let key = Digest::of(b"key");
    kept(&writer, &key, &lib(Source::Commit(blob(9))));
    // A commit of the same tree reads the same decision.
    let other = lib(Source::Commit(blob(8)));
    let read = writer.references(&key, &other).unwrap();
    assert_eq!(read, grouped(&other));
}

/// The references kept under one key after the row of `tasks/a` is set to
/// `value` in the file, or the code and message reading them fails with.
fn read_with(value: &str) -> Result<Vec<KeptReferences>, (Code, String)> {
    let cache = tempfile::tempdir().unwrap();
    let key = Digest::of(b"key");
    let own = lib(Source::Commit(blob(9)));
    let open = || Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = open().write().unwrap();
    kept(&writer, &key, &own);
    writer.decide(Vec::new()).unwrap();
    let file = Connection::open(location(cache.path(), Path::new(ROOT))).unwrap();
    let sql = "UPDATE resolutions SET reached = ?1 WHERE path = 'tasks/a'";
    file.execute(sql, [value]).unwrap();
    drop(file);
    let writer = open().write().unwrap();
    (writer.references(&key, &own)).map_err(|error| (error.code(), error.to_string()))
}

#[test]
fn a_kept_reference_that_cannot_be_read_fails_with_the_index_code_and_its_twin_reads() {
    let admitted = r#"[{"pointer": "/dir", "target": "/projects/q", "location": {"path": "projects", "object": null, "board": [null, "worktree"]}, "code": "board::ref::dangling"}]"#;
    let read = read_with(admitted).unwrap();
    assert_eq!(
        read[1].1[0].location.as_ref().unwrap().board.source,
        Source::WorkingTree
    );
    for refused in [
        r#"[{"pointer": "/a", "target": "no/slash"}]"#,
        r#"[{"pointer": "/a", "target": "/a", "code": "board::ref::lost"}]"#,
        r#"[{"pointer": "/a", "target": "/a", "location": {"object": null}}]"#,
        r#"[{"pointer": "/a", "target": "/a", "location": {"path": "p", "object": null, "board": [null]}}]"#,
        r#"[{"pointer": "/a", "target": "/a", "location": {"path": "p", "object": "nothex"}}]"#,
        r#"[{"pointer": "/a", "target": "/a", "extra": 1}]"#,
        "not json",
    ] {
        assert_eq!(
            read_with(refused),
            Err((
                Code::IndexFailed,
                format!("the index keeps reached {refused:?} for tasks/a, which cannot be read")
            )),
            "{refused}"
        );
    }
}

#[test]
fn a_prune_deletes_the_references_of_a_decision_no_source_reached_and_keeps_those_of_one_it_did() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let [reached, gone] = [b"reached".as_slice(), b"gone"].map(Digest::of);
    let own = lib(Source::Commit(blob(9)));
    for key in [&reached, &gone] {
        kept(&writer, key, &own);
    }
    writer.reach(&own.source, Some(&reached), None).unwrap();
    writer.prune().unwrap();
    assert_eq!(writer.references(&reached, &own).unwrap(), grouped(&own));
    assert_eq!(writer.references(&gone, &own).unwrap(), Vec::new());
}

#[test]
fn forgetting_paths_of_a_slot_deletes_their_references_and_keeps_the_others() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    let slot = Digest::of(b"slot");
    let own = lib(Source::WorkingTree);
    kept(&writer, &slot, &own);
    writer.forget(&slot, Some(&["tasks/a"])).unwrap();
    let paths: Vec<String> = (writer.references(&slot, &own).unwrap().into_iter())
        .map(|(path, _)| path)
        .collect();
    assert_eq!(paths, ["notes.md"]);
    writer.forget(&slot, None).unwrap();
    assert_eq!(writer.references(&slot, &own).unwrap(), Vec::new());
}

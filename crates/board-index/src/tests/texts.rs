//! The text index: what `texts_fts` holds as texts are stored, and the
//! modes of the files that keep the prose.

use std::os::unix::fs::PermissionsExt as _;

use super::*;

fn text(heading: Option<&str>, body: &str) -> board_model::Text {
    board_model::Text {
        heading: heading.map(str::to_owned),
        body: body.to_owned(),
    }
}

/// The ids of the texts `texts_fts` finds for the FTS5 query `query`.
fn matched(connection: &Connection, query: &str) -> Vec<i64> {
    connection
        .prepare("SELECT rowid FROM main.texts_fts WHERE texts_fts MATCH ?1 ORDER BY rowid")
        .unwrap()
        .query_map([query], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The id `main.texts` gives the text of `blob`.
fn id(connection: &Connection, blob: ObjectId) -> i64 {
    connection
        .query_row(
            "SELECT id FROM main.texts WHERE blob = ?1",
            [blob.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_stored_text_is_indexed_once_under_its_id_and_found_by_any_case_of_three_characters() {
    let writer = Index::fall_back().unwrap().write().unwrap();
    writer
        .put_text(blob(1), &text(Some("Lighthouse"), "# Lighthouse keeper"))
        .unwrap();
    writer.put_text(blob(1), &text(None, "kettle")).unwrap();
    writer.put_text(blob(2), &text(None, "a kettle")).unwrap();
    let connection = writer.connection();
    let indexed: i64 = connection
        .query_row("SELECT count(*) FROM main.texts_fts_docsize", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(indexed, 2);
    let (one, two) = (id(connection, blob(1)), id(connection, blob(2)));
    assert_eq!(matched(connection, "\"HOUSE kee\""), [one]);
    assert_eq!(matched(connection, "\"ttl\""), [two]);
    assert_eq!(matched(connection, "\"keepers\""), Vec::<i64>::new());
}

#[test]
fn a_text_keeps_its_id_and_its_trigrams_through_a_full_vacuum() {
    let index = Index::fall_back().unwrap();
    let writer = index.write().unwrap();
    for (byte, body) in [
        (1, "first lantern"),
        (2, "second kettle"),
        (3, "third orchard"),
    ] {
        writer.put_text(blob(byte), &text(None, body)).unwrap();
    }
    writer
        .connection()
        .execute(
            "DELETE FROM main.texts WHERE blob = ?1",
            [blob(1).to_string()],
        )
        .unwrap();
    let index = writer.decide(Vec::new()).unwrap().index;
    let before = (
        id(&index.connection, blob(2)),
        id(&index.connection, blob(3)),
    );
    index.connection.execute_batch("VACUUM").unwrap();
    let after = (
        id(&index.connection, blob(2)),
        id(&index.connection, blob(3)),
    );
    assert_eq!(after, before);
    assert_eq!(matched(&index.connection, "\"orchard\""), [after.1]);
    assert_eq!(matched(&index.connection, "\"lantern\""), Vec::<i64>::new());
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The index file with `suffix`, such as `-wal`.
fn journal(file: &Path, suffix: &str) -> PathBuf {
    let mut path = file.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

#[test]
fn the_index_file_and_its_journals_are_created_readable_only_by_their_owner_in_a_private_directory()
{
    let cache = tempfile::tempdir().unwrap();
    let view = filled(cache.path());
    let file = location(cache.path(), Path::new(ROOT));
    for suffix in ["", "-wal", "-shm"] {
        assert_eq!(mode(&journal(&file, suffix)), 0o600, "{suffix}");
    }
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    drop(view);
    overwrite(
        &file,
        b"this is not a database, and it is long enough to be read",
    );
    let rebuilt = filled(cache.path());
    for suffix in ["", "-wal", "-shm"] {
        assert_eq!(mode(&journal(&file, suffix)), 0o600, "rebuilt {suffix}");
    }
    drop(rebuilt);
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test puts files where the cache keeps them"
)]
#[test]
fn the_other_files_and_directories_in_the_cache_keep_their_modes() {
    let cache = tempfile::tempdir().unwrap();
    let file = location(cache.path(), Path::new(ROOT));
    let directory = file.parent().unwrap();
    std::fs::create_dir_all(directory).unwrap();
    let stray = directory.join("notes");
    std::fs::write(&stray, b"kept").unwrap();
    std::fs::set_permissions(&stray, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    let made = cache.path().join("made");
    std::fs::create_dir(&made).unwrap();
    let view = filled(cache.path());
    assert_eq!(mode(&stray), 0o644);
    assert_eq!(mode(directory), 0o755, "a directory that was there is left");
    assert_eq!(mode(&cache.path().join("board")), mode(&made));
    assert_eq!(mode(&directory.join("root")), 0o600);
    drop(view);
}

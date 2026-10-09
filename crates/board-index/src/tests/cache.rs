//! The cache directory: what creating an index deletes and keeps beside it.

use super::*;
use std::time::Duration;

use crate::open::databases_first;

/// Opens an index for a board whose root is a directory that `gone`
/// removes, then opens one for another board, after opening it once first
/// when `reused`. Returns whether the first board's cache is still there.
fn kept(gone: bool, reused: bool) -> bool {
    let cache = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let file = location(cache.path(), first.path());
    drop(Index::open(Some(cache.path()), first.path()).unwrap());
    if reused {
        drop(Index::open(Some(cache.path()), second.path()).unwrap());
    }
    if gone {
        drop(first);
    }
    drop(Index::open(Some(cache.path()), second.path()).unwrap());
    file.parent().unwrap().exists()
}

#[test]
fn creating_an_index_deletes_the_cache_of_a_board_whose_root_is_gone() {
    assert!(!kept(true, false));
}

#[test]
fn creating_an_index_keeps_the_cache_of_a_board_whose_root_exists() {
    assert!(kept(false, false));
}

#[test]
fn reusing_an_index_keeps_the_cache_of_a_board_whose_root_is_gone() {
    assert!(kept(true, true));
}

/// Writes `bytes` to `path`, making its directory.
#[expect(
    clippy::disallowed_methods,
    reason = "a test puts files where the cache keeps them"
)]
fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Deletes the file at `path`, as another process can.
#[expect(
    clippy::disallowed_methods,
    reason = "a test deletes files where the cache keeps them"
)]
fn remove(path: &Path) {
    std::fs::remove_file(path).unwrap();
}

/// Puts a cache directory for another board, recording a root that does not
/// exist when `recorded`, then creates an index. Returns whether that
/// directory is still there.
fn unrecorded_kept(recorded: bool) -> bool {
    let cache = tempfile::tempdir().unwrap();
    let other = cache.path().join("board/other");
    put(&other.join(format!("index-{INDEX_VERSION}.sqlite")), b"");
    if recorded {
        put(&other.join("root"), b"/no/such/board");
    }
    drop(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    other.exists()
}

#[test]
fn creating_an_index_keeps_a_cache_directory_that_records_no_root() {
    assert!(unrecorded_kept(false));
}

#[test]
fn creating_an_index_deletes_a_cache_directory_whose_recorded_root_is_gone() {
    assert!(!unrecorded_kept(true));
}

#[test]
fn creating_an_index_keeps_the_timing_logs_beside_a_cache_directory_it_deletes() {
    let cache = tempfile::tempdir().unwrap();
    let other = cache.path().join("board/other");
    put(&other.join("root"), b"/no/such/board");
    let logs = ["board/timings.jsonl", "board/timings.jsonl.1"].map(|log| cache.path().join(log));
    for log in &logs {
        put(log, b"{}\n");
    }
    drop(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    assert!(
        !other.exists(),
        "the cache directory of a board that is gone is deleted"
    );
    for log in &logs {
        assert!(log.is_file(), "{} is kept", log.display());
    }
}

/// Opens an index for a board inside a directory, removes the board, takes
/// away permission to look inside that directory when `hidden`, then
/// creates an index for another board. Returns whether the first board's
/// cache is still there.
fn hidden_kept(hidden: bool) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    let cache = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir_in(parent.path()).unwrap();
    let file = location(cache.path(), first.path());
    drop(Index::open(Some(cache.path()), first.path()).unwrap());
    drop(first);
    let mode = if hidden { 0o000 } else { 0o700 };
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    drop(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    file.parent().unwrap().exists()
}

#[test]
fn creating_an_index_keeps_the_cache_of_a_board_whose_root_cannot_be_looked_at() {
    assert!(hidden_kept(true));
}

#[test]
fn creating_an_index_deletes_the_cache_of_a_board_whose_root_can_be_seen_to_be_gone() {
    assert!(!hidden_kept(false));
}

/// The names of the files in `directory`, sorted.
fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Puts files of older index formats beside the index for `ROOT`, after
/// opening it once first when `reused`, then opens it. Returns the index
/// files left that are not of the current format.
fn formats_left(reused: bool) -> Vec<String> {
    let cache = tempfile::tempdir().unwrap();
    let file = location(cache.path(), Path::new(ROOT));
    let directory = file.parent().unwrap();
    if reused {
        drop(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    }
    let (previous, older) = (INDEX_VERSION - 1, INDEX_VERSION - 2);
    for name in [
        "index.sqlite".to_owned(),
        "index.sqlite-wal".to_owned(),
        format!("index-{older}.sqlite"),
        format!("index-{older}.sqlite-shm"),
        format!("index-{previous}.sqlite"),
        format!("index-{previous}.sqlite-wal"),
    ] {
        put(&directory.join(name), b"");
    }
    drop(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    let current = format!("index-{INDEX_VERSION}.");
    names(directory)
        .into_iter()
        .filter(|name| name.starts_with("index") && !name.starts_with(&current))
        .collect()
}

#[test]
fn creating_an_index_deletes_files_older_than_the_previous_format() {
    let previous = INDEX_VERSION - 1;
    assert_eq!(
        formats_left(false),
        [
            format!("index-{previous}.sqlite"),
            format!("index-{previous}.sqlite-wal"),
        ]
    );
}

#[test]
fn reusing_an_index_keeps_the_files_of_every_format() {
    assert_eq!(formats_left(true).len(), 6);
}

#[test]
fn databases_are_deleted_before_the_journals() {
    let paths = ["a.sqlite-wal", "a.sqlite", "b.sqlite-shm", "b.sqlite"].map(PathBuf::from);
    assert_eq!(
        databases_first(paths.to_vec()),
        ["a.sqlite", "b.sqlite", "a.sqlite-wal", "b.sqlite-shm"].map(PathBuf::from)
    );
}

/// The `plugins` directory beside an index kept in a file.
fn plugins(storage: &Storage) -> PathBuf {
    let Storage::File(file) = storage else {
        panic!("the index is in memory");
    };
    file.parent().unwrap().join("plugins")
}

/// Puts databases of several versions of plugin `p`, and one of plugin `q`,
/// beside an index kept in a file, after opening `p`'s database at `version`
/// once, then opens that database again. Returns the databases left.
fn databases_left(version: &str) -> Vec<String> {
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let first = PluginDatabase::open(index.storage(), "p", version).unwrap();
    let plugins = plugins(index.storage());
    for name in [
        "p@0.9.10.sqlite",
        "p@1.2.0.sqlite",
        "p@1.2.0.sqlite-wal",
        "p@2.0.0.sqlite",
        "q@1.0.0.sqlite",
    ] {
        if !plugins.join(name).exists() {
            put(&plugins.join(name), b"");
        }
    }
    let opened = PluginDatabase::open(index.storage(), "p", version).unwrap();
    assert!(!opened.fallback());
    assert_eq!(opened.id(), first.id(), "the opened database was kept");
    names(&plugins)
}

#[test]
fn opening_a_plugins_database_deletes_those_of_its_older_versions() {
    assert_eq!(
        databases_left("1.10.0"),
        ["p@1.10.0.sqlite", "p@2.0.0.sqlite", "q@1.0.0.sqlite"]
    );
}

#[test]
fn opening_a_plugins_oldest_database_deletes_none() {
    assert_eq!(
        databases_left("0.9.10"),
        [
            "p@0.9.10.sqlite",
            "p@1.2.0.sqlite",
            "p@1.2.0.sqlite-wal",
            "p@2.0.0.sqlite",
            "q@1.0.0.sqlite",
        ]
    );
}

/// The number of blobs the index records plugin `p` at 1.0.0 handled.
fn handled(view: &View) -> i64 {
    view.index
        .connection
        .query_row("SELECT count(*) FROM main.extracted", [], |row| row.get(0))
        .unwrap()
}

/// Opens plugin `p`'s database, trusts it and records a blob handled into
/// it, then opens it again, after deleting it and making it again empty at
/// the same path when `remade`, as another version's build and this
/// version's plugin can. Returns whether the second opening kept the id,
/// and the blobs the index then counts as handled.
fn trusted_after(remade: bool) -> (bool, i64) {
    let cache = tempfile::tempdir().unwrap();
    let index = decided(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    let first = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", first.id().unwrap()).unwrap();
    index
        .handled("p", "1.0.0", first.id().unwrap(), &[blob(1)])
        .unwrap();
    if remade {
        let file = plugins(index.storage()).join("p@1.0.0.sqlite");
        remove(&file);
        put(&file, b"");
    }
    let second = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", second.id().unwrap()).unwrap();
    (first.id() == second.id(), handled(&index))
}

#[test]
fn a_plugin_database_made_again_at_its_path_makes_the_index_forget_what_it_handled() {
    assert_eq!(trusted_after(true), (false, 0));
}

#[test]
fn a_plugin_database_opened_again_keeps_its_id_and_what_it_handled() {
    assert_eq!(trusted_after(false), (true, 1));
}

/// Opens plugin `p`'s database and trusts it, then deletes it and opens one
/// made in its place and trusts that, then records a blob handled into the
/// first one when `stale`, or the second. Returns the blobs the index then
/// counts as handled.
fn handled_into(stale: bool) -> i64 {
    let cache = tempfile::tempdir().unwrap();
    let index = decided(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    let first = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", first.id().unwrap()).unwrap();
    remove(&plugins(index.storage()).join("p@1.0.0.sqlite"));
    let second = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", second.id().unwrap()).unwrap();
    let into = if stale { &first } else { &second };
    index
        .handled("p", "1.0.0", into.id().unwrap(), &[blob(1)])
        .unwrap();
    handled(&index)
}

#[test]
fn blobs_handled_into_a_database_the_index_no_longer_trusts_are_not_recorded() {
    assert_eq!(handled_into(true), 0);
}

#[test]
fn blobs_handled_into_the_database_the_index_trusts_are_recorded() {
    assert_eq!(handled_into(false), 1);
}

/// Trusts plugin `p`'s database, then, while another connection holds the
/// index's write lock, trusts it again, or one made in its place when
/// `remade`. Returns whether the second trust waited for the lock.
fn trust_waited(remade: bool) -> bool {
    let cache = tempfile::tempdir().unwrap();
    let index = decided(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    let first = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", first.id().unwrap()).unwrap();
    if remade {
        remove(&plugins(index.storage()).join("p@1.0.0.sqlite"));
    }
    let second = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    let Storage::File(file) = index.storage() else {
        panic!("the index is in memory");
    };
    let holder = Connection::open(file).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    index.index.connection.busy_timeout(Duration::ZERO).unwrap();
    matches!(
        index.trust("p", "1.0.0", second.id().unwrap()),
        Err(IndexError::Busy { .. })
    )
}

#[test]
fn trusting_the_recorded_database_does_not_wait_for_another_process_write() {
    assert!(!trust_waited(false));
}

#[test]
fn trusting_a_database_made_again_waits_for_another_process_write() {
    assert!(trust_waited(true));
}

/// How another process can keep Board from writing a plugin's database.
#[derive(Clone, Copy)]
enum Unwritable {
    /// The plugin holds the database's write lock.
    Locked,
    /// The file is read-only.
    ReadOnly,
}

/// Opens plugin `p`'s database, trusts it and records a blob handled into
/// it. Then, after deleting it and making it again at the same path when
/// `remade`, keeps Board from writing it as `unwritable` says, opens it
/// again and trusts the id it reads. Returns whether the second opening read
/// the first's id, and the blobs the index then counts as handled.
fn handled_while_unwritable(unwritable: Unwritable, remade: bool) -> (bool, i64) {
    use std::os::unix::fs::PermissionsExt as _;
    let cache = tempfile::tempdir().unwrap();
    let index = decided(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    let first = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    index.trust("p", "1.0.0", first.id().unwrap()).unwrap();
    index
        .handled("p", "1.0.0", first.id().unwrap(), &[blob(1)])
        .unwrap();
    let file = plugins(index.storage()).join("p@1.0.0.sqlite");
    if remade {
        remove(&file);
        drop(PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap());
    }
    let holder = Connection::open(&file).unwrap();
    match unwritable {
        Unwritable::Locked => holder
            .execute_batch("BEGIN IMMEDIATE; CREATE TABLE tasks (id TEXT)")
            .unwrap(),
        Unwritable::ReadOnly => {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        }
    }
    let second = PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap();
    let id = second.id().expect("an id is read without writing");
    index.trust("p", "1.0.0", id).unwrap();
    (second.id() == first.id(), handled(&index))
}

#[test]
fn a_plugin_database_whose_write_lock_the_plugin_holds_keeps_what_it_handled() {
    assert_eq!(
        handled_while_unwritable(Unwritable::Locked, false),
        (true, 1)
    );
}

#[test]
fn a_plugin_database_made_again_whose_write_lock_the_plugin_holds_forgets_what_it_handled() {
    assert_eq!(
        handled_while_unwritable(Unwritable::Locked, true),
        (false, 0)
    );
}

#[test]
fn a_read_only_plugin_database_keeps_what_it_handled() {
    assert_eq!(
        handled_while_unwritable(Unwritable::ReadOnly, false),
        (true, 1)
    );
}

#[test]
fn a_read_only_plugin_database_made_again_forgets_what_it_handled() {
    assert_eq!(
        handled_while_unwritable(Unwritable::ReadOnly, true),
        (false, 0)
    );
}

/// Makes the `plugins` directory read-only, after opening plugin `p`'s
/// database once when `opened`, then opens it. Returns what that opening
/// failed with, if it failed, and the path it named from the `plugins`
/// directory.
fn opened_in_read_only_directory(opened: bool) -> Option<(std::io::ErrorKind, PathBuf)> {
    use std::os::unix::fs::PermissionsExt as _;
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let plugins = plugins(index.storage());
    // Another plugin's database makes the directory in both cases.
    put(&plugins.join("q@1.0.0.sqlite"), b"");
    if opened {
        drop(PluginDatabase::open(index.storage(), "p", "1.0.0").unwrap());
    }
    std::fs::set_permissions(&plugins, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = PluginDatabase::open(index.storage(), "p", "1.0.0");
    std::fs::set_permissions(&plugins, std::fs::Permissions::from_mode(0o755)).unwrap();
    result.err().map(|error| match error {
        DatabaseError::File { path, source } => (
            source.kind(),
            path.strip_prefix(&plugins).unwrap().to_path_buf(),
        ),
        DatabaseError::TemporaryDirectory { .. } => panic!("{error}"),
    })
}

#[test]
fn a_plugin_database_that_cannot_be_created_is_refused() {
    assert_eq!(
        opened_in_read_only_directory(false),
        Some((
            std::io::ErrorKind::PermissionDenied,
            PathBuf::from("p@1.0.0.sqlite")
        ))
    );
}

#[test]
fn a_plugin_database_already_created_opens_in_a_read_only_directory() {
    assert_eq!(opened_in_read_only_directory(true), None);
}

/// Trusts plugin `p`'s database at 1.0.0 and records a blob handled into
/// it, then trusts a database for `plugin` at `version`, then trusts `p`'s
/// again. Returns the blobs the index then counts as handled.
fn handled_after_trusting(plugin: &str, version: &str) -> i64 {
    let cache = tempfile::tempdir().unwrap();
    let index = decided(Index::open(Some(cache.path()), Path::new(ROOT)).unwrap());
    index.trust("p", "1.0.0", "a").unwrap();
    index.handled("p", "1.0.0", "a", &[blob(1)]).unwrap();
    index.trust(plugin, version, "b").unwrap();
    index.trust("p", "1.0.0", "a").unwrap();
    handled(&index)
}

#[test]
fn trusting_another_plugins_database_keeps_what_a_plugin_handled() {
    assert_eq!(handled_after_trusting("q", "1.0.0"), 1);
    assert_eq!(handled_after_trusting("p", "2.0.0"), 1);
}

#[test]
fn trusting_another_database_for_the_same_plugin_version_forgets_what_it_handled() {
    assert_eq!(handled_after_trusting("p", "1.0.0"), 0);
}

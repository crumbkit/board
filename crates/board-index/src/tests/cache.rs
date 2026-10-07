//! The cache directory: what creating an index deletes and keeps beside it.

use super::*;

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

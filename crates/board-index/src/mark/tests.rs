#![expect(
    clippy::disallowed_methods,
    reason = "the tests put files where the cache keeps them and make its root unwritable"
)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use board_model::Code;

use super::*;
use crate::{Index, Storage, append_timing, location};

const SIGNATURE: &str = "Signature: 8a477f597d28d172789f06886806bc55\n";

fn tag(cache: &Path) -> Vec<u8> {
    std::fs::read(cache.join("board/CACHEDIR.TAG")).unwrap()
}

/// Makes `directory` readable but not writable by its owner when `locked`.
fn lock(directory: &Path, locked: bool) {
    let mode = if locked { 0o500 } else { 0o700 };
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn digest(log: &[u8], _: &[u8]) -> Vec<u8> {
    log.to_vec()
}

#[test]
fn opening_an_index_in_a_new_cache_root_tags_the_root_as_a_cache() {
    let cache = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    drop(Index::open(Some(cache.path()), root.path()).unwrap());
    let tag = String::from_utf8(tag(cache.path())).unwrap();
    assert!(tag.starts_with(SIGNATURE), "{tag}");
    assert!(
        tag.lines().skip(1).all(|line| line.starts_with('#')),
        "{tag}"
    );
    assert!(tag.contains("Board"), "{tag}");
    let board = location(cache.path(), root.path());
    assert!(!board.parent().unwrap().join("CACHEDIR.TAG").exists());
    assert!(take_unmarked().is_empty());
}

#[test]
fn opening_an_index_leaves_an_existing_tag_byte_for_byte_unchanged() {
    let cache = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(cache.path().join("board")).unwrap();
    let mine = format!("{SIGNATURE}# Someone else's tag.\n");
    std::fs::write(cache.path().join("board/CACHEDIR.TAG"), &mine).unwrap();
    drop(Index::open(Some(cache.path()), root.path()).unwrap());
    assert_eq!(tag(cache.path()), mine.as_bytes());
}

#[test]
fn appending_a_timing_line_in_a_new_cache_root_tags_the_root_as_a_cache() {
    let cache = tempfile::tempdir().unwrap();
    append_timing(cache.path(), b"{}\n", &digest).unwrap();
    assert!(tag(cache.path()).starts_with(SIGNATURE.as_bytes()));
}

/// Opens an index for a board whose cache directory exists, under a cache
/// root that is unwritable when `locked`, and gives what was warned.
fn opened_warnings(locked: bool) -> (Storage, Vec<Unmarked>) {
    let cache = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let file = location(cache.path(), root.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let board = cache.path().join("board");
    lock(&board, locked);
    let index = Index::open(Some(cache.path()), root.path()).unwrap();
    let storage = index.storage().clone();
    drop(index);
    lock(&board, false);
    (storage, take_unmarked())
}

#[test]
fn an_unwritable_cache_root_warns_that_it_is_unmarked_and_the_index_still_opens_on_file() {
    let cache_file = |storage: &Storage| matches!(storage, Storage::File(_));
    let (storage, warned) = opened_warnings(true);
    assert!(cache_file(&storage), "{storage:?}");
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert_eq!(warned[0].code(), Code::CacheUnmarked);
    assert_eq!(Code::CacheUnmarked.as_str(), "board::cache::unmarked");
    let message = warned[0].to_string();
    assert!(message.contains("/board"), "{message}");
    assert!(message.contains("denied"), "{message}");
}

#[test]
fn a_writable_cache_root_gives_no_warning() {
    let (storage, warned) = opened_warnings(false);
    assert!(matches!(storage, Storage::File(_)), "{storage:?}");
    assert!(warned.is_empty(), "{warned:?}");
}

/// Appends a timing line to a log that exists, under a cache root that is
/// unwritable when `locked`, and gives what was warned.
fn appended_warnings(locked: bool) -> Vec<Unmarked> {
    let cache = tempfile::tempdir().unwrap();
    let board = cache.path().join("board");
    std::fs::create_dir_all(&board).unwrap();
    std::fs::write(board.join("timings.jsonl"), b"").unwrap();
    lock(&board, locked);
    append_timing(cache.path(), b"{}\n", &digest).unwrap();
    lock(&board, false);
    assert_eq!(std::fs::read(board.join("timings.jsonl")).unwrap(), b"{}\n");
    take_unmarked()
}

#[test]
fn an_unwritable_cache_root_warns_once_and_the_timing_line_is_still_appended() {
    let warned = appended_warnings(true);
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert_eq!(warned[0].code(), Code::CacheUnmarked);
}

#[test]
fn a_writable_cache_root_appends_without_a_warning() {
    assert!(appended_warnings(false).is_empty());
}

#[test]
fn a_root_warned_about_once_is_not_warned_about_again_in_the_same_run() {
    let cache = tempfile::tempdir().unwrap();
    let board = cache.path().join("board");
    std::fs::create_dir_all(&board).unwrap();
    lock(&board, true);
    mark(&board);
    mark(&board);
    let first = take_unmarked();
    mark(&board);
    lock(&board, false);
    assert_eq!((first.len(), take_unmarked().len()), (1, 0));
}

#[cfg(target_os = "macos")]
mod time_machine {
    use std::process::Command;

    use super::*;

    const ATTRIBUTE: &str = "com.apple.metadata:com_apple_backup_excludeItem";

    fn attribute(directory: &Path) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        rustix::fs::getxattr(directory, ATTRIBUTE, &mut bytes).unwrap();
        bytes
    }

    /// Whether `tmutil isexcluded` reports `directory` excluded.
    fn excluded(directory: &Path) -> bool {
        let output = Command::new("tmutil")
            .arg("isexcluded")
            .arg(directory)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).starts_with("[Excluded]")
    }

    #[test]
    fn a_new_cache_root_is_excluded_from_time_machine_and_an_unmarked_twin_is_not() {
        // Time Machine already leaves out the system's temporary
        // directories, so the cache sits in the crate's directory instead.
        let cache = tempfile::Builder::new()
            .prefix(".mark-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        std::fs::create_dir(cache.path().join("twin")).unwrap();
        append_timing(cache.path(), b"{}\n", &digest).unwrap();
        assert!(excluded(&cache.path().join("board")));
        assert!(!excluded(&cache.path().join("twin")));
    }

    #[test]
    fn the_exclusion_is_the_bytes_tmutil_addexclusion_writes() {
        let cache = tempfile::tempdir().unwrap();
        append_timing(cache.path(), b"{}\n", &digest).unwrap();
        let by_tmutil = tempfile::tempdir().unwrap();
        let status = Command::new("tmutil")
            .arg("addexclusion")
            .arg(by_tmutil.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            attribute(&cache.path().join("board")),
            attribute(by_tmutil.path())
        );
    }
}

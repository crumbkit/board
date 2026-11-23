#![expect(
    clippy::disallowed_methods,
    reason = "the tests make the directories the files they write go in"
)]

use std::path::Path;

use super::*;
#[test]
fn a_held_lock_refuses_a_second_holder_and_is_free_once_dropped() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("state").join("run.lock");
    let held = lock_state(&file).unwrap();
    assert!(held.is_some());
    assert!(lock_state(&file).unwrap().is_none());
    drop(held);
    assert!(lock_state(&file).unwrap().is_some());
}

#[test]
fn an_appended_log_rotates_above_its_limit_and_keeps_appending() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("logs").join("a.log");
    append_state(&file, "one", 4).unwrap();
    append_state(&file, "two", 100).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
    append_state(&file, "three", 4).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "three\n");
    let rotated = directory.path().join("logs").join("a.log.1");
    assert_eq!(std::fs::read_to_string(rotated).unwrap(), "one\ntwo\n");
    let mode = std::fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_replaced_file_has_its_mode_and_leaves_its_directory_mode() {
    let directory = tempfile::tempdir().unwrap();
    let shared = directory.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = shared.join("agent.plist");
    replace_file(&file, b"one", 0o644).unwrap();
    replace_file(&file, b"two", 0o644).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"two");
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o644);
    assert_eq!(mode(&shared), 0o755);
}

#[test]
fn leftover_scratch_directories_are_removed_and_others_kept() {
    let directory = tempfile::tempdir().unwrap();
    let left = scratch_directory(directory.path(), "scan-").unwrap().keep();
    let other = directory.path().join("other");
    std::fs::create_dir(&other).unwrap();
    remove_scratch(directory.path(), "scan-").unwrap();
    assert!(!left.exists());
    assert!(other.exists());
    remove_scratch(&directory.path().join("absent"), "scan-").unwrap();
}

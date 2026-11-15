use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use board_scenario::{Item, Scenario};

use crate::{Repository, WriteError};

/// A scenario whose `index.lock` was left by a killed Board process: the
/// lock file exists, and the record names it when `named` is true.
fn leftover(named: bool) -> (Scenario, Repository) {
    let scenario = Scenario::new();
    scenario.identified().commit(&[("a.json", "{}")]);
    scenario.write(".git/index.lock", Item::File(""));
    let repository = Repository::open(scenario.workdir()).unwrap();
    if named {
        let owned = super::Owned::claim(
            &repository.git_dir().join("board-locks"),
            &[&repository.git_dir().join("index.lock")],
        );
        std::mem::forget(owned);
    }
    (scenario, repository)
}

fn staging(repository: &Repository) -> Result<(), WriteError> {
    let lock = repository.lock(Duration::from_millis(100))?;
    repository
        .stage_commit(&lock, &[("b.json", Some(b"{}\n"))], "board put")
        .map(|_| ())
}

#[test]
fn an_index_lock_the_record_names_is_removed_and_the_entry_with_it() {
    let (scenario, repository) = leftover(true);
    staging(&repository).unwrap();
    assert!(!scenario.workdir().join(".git/index.lock").exists());
    assert!(!scenario.workdir().join(".git/board-locks").exists());
}

#[test]
fn an_index_lock_with_no_record_is_refused_naming_the_file() {
    let (scenario, repository) = leftover(false);
    let error = staging(&repository).unwrap_err();
    let held = repository.git_dir().join("index.lock");
    assert!(
        matches!(&error, WriteError::Locked { lock } if *lock == held),
        "{error:?}"
    );
    assert!(scenario.workdir().join(".git/index.lock").exists());
}

#[test]
fn an_index_lock_with_another_inode_than_the_record_names_is_refused_and_kept() {
    let (scenario, repository) = leftover(true);
    // Another process makes its own lock file while the recorded one still
    // exists, so the two inodes differ, and renames it into place.
    scenario.write(".git/other.lock", Item::File(""));
    std::fs::rename(
        scenario.workdir().join(".git/other.lock"),
        scenario.workdir().join(".git/index.lock"),
    )
    .unwrap();
    let error = staging(&repository).unwrap_err();
    assert!(matches!(error, WriteError::Locked { .. }), "{error:?}");
    assert!(scenario.workdir().join(".git/index.lock").exists());
}

#[test]
fn an_index_lock_with_the_recorded_inode_and_another_birth_time_is_refused_and_kept() {
    let (scenario, repository) = leftover(true);
    let record = repository.git_dir().join("board-locks");
    // A file system that reuses inode numbers gives the file a person
    // recreated the recorded device and inode, and a birth time of its own.
    let reborn = |metadata: &std::fs::Metadata| {
        let found = super::Identity::of(metadata);
        super::Identity {
            birth: found.birth.map(|(seconds, nanos)| (seconds, nanos ^ 1)),
            ..found
        }
    };
    super::clear_with(&record, reborn).unwrap();
    assert!(scenario.workdir().join(".git/index.lock").exists());
    let error = staging(&repository).unwrap_err();
    let held = repository.git_dir().join("index.lock");
    assert!(
        matches!(&error, WriteError::Locked { lock } if *lock == held),
        "{error:?}"
    );
}

#[test]
fn an_entry_with_no_birth_time_keeps_its_lock_file_and_is_removed() {
    for text in [
        // The earlier format, with no birth time.
        "{device} {inode} {path}\n",
        // The current format where the file system gave none.
        "{device} {inode} - {path}\n",
    ] {
        let (scenario, repository) = leftover(false);
        let path = repository.git_dir().join("index.lock");
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        let record = repository.git_dir().join("board-locks");
        let text = text
            .replace("{device}", &metadata.dev().to_string())
            .replace("{inode}", &metadata.ino().to_string())
            .replace("{path}", path.to_str().unwrap());
        std::fs::write(&record, text).unwrap();
        let error = staging(&repository).unwrap_err();
        assert!(matches!(error, WriteError::Locked { .. }), "{error:?}");
        assert!(scenario.workdir().join(".git/index.lock").exists());
        assert!(!record.exists());
    }
}

#[test]
fn a_recorded_lock_file_that_cannot_be_removed_is_named_in_the_refusal() {
    let scenario = Scenario::new();
    scenario.identified().commit(&[("a.json", "{}")]);
    // A directory cannot be removed as a file.
    std::fs::create_dir(scenario.workdir().join(".git/index.lock")).unwrap();
    let repository = Repository::open(scenario.workdir()).unwrap();
    let held = repository.git_dir().join("index.lock");
    std::mem::forget(super::Owned::claim(
        &repository.git_dir().join("board-locks"),
        &[&held],
    ));
    let error = staging(&repository).unwrap_err();
    assert!(
        matches!(&error, WriteError::Io { path, .. } if *path == held),
        "{error:?}"
    );
}

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

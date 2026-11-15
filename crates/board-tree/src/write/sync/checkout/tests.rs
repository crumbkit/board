use std::time::Duration;

use board_scenario::{Item, Scenario};

use crate::{Deadline, GIT_DEADLINE, Repository};

/// A remote and a clone one commit behind it, able to commit.
fn behind() -> (Scenario, Scenario) {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    remote.commit(&[("a.json", "0")]);
    let clone = Scenario::clone_of(&remote);
    clone.identified();
    remote.commit(&[("a.json", "1")]);
    (remote, clone)
}

fn sync(clone: &Scenario) {
    Repository::open(clone.workdir())
        .unwrap()
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap();
}

#[test]
fn a_record_naming_a_commit_the_store_lacks_is_ignored_and_removed() {
    let (_remote, clone) = behind();
    clone.write(
        ".git/board-checkout",
        Item::File("0123456789012345678901234567890123456789\nnot a commit\n"),
    );
    sync(&clone);
    assert!(!clone.workdir().join(".git/board-checkout").exists());
    assert_eq!(clone.head().unwrap().files["a.json"], b"1");
}

#[test]
fn a_record_naming_the_commit_head_holds_is_removed_and_the_sync_goes_on() {
    let (_remote, clone) = behind();
    let head = clone.head().unwrap().id;
    clone.write(".git/board-checkout", Item::File(&format!("{head}\n")));
    sync(&clone);
    assert!(!clone.workdir().join(".git/board-checkout").exists());
    assert_eq!(clone.head().unwrap().files["a.json"], b"1");
}

#[test]
fn a_record_of_the_earlier_format_accepts_no_blob_of_its_commit() {
    let remote = Scenario::new();
    remote.identified().accepting_pushes();
    let first = remote.commit(&[("a.json", "0")]);
    let clone = Scenario::clone_of(&remote);
    clone.identified();
    let second = remote.commit(&[("a.json", "1")]);
    remote.commit(&[("a.json", "2")]);
    // The record names only the commit a checkout was moving to, so the sync
    // cannot tell which files it wrote, and a file that holds that commit's
    // blob is a person's edit.
    assert_ne!(first, second);
    clone.write(".git/board-checkout", Item::File(&format!("{second}\n")));
    clone.write("a.json", Item::Fresh("1"));
    let error = Repository::open(clone.workdir())
        .unwrap()
        .sync(false, Deadline::after(GIT_DEADLINE), Duration::ZERO)
        .unwrap_err();
    assert_eq!(error.code(), board_model::Code::SyncEdited, "{error}");
    assert_eq!(std::fs::read(clone.workdir().join("a.json")).unwrap(), b"1");
}

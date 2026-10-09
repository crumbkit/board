//! The scenario builder cleans up after itself.

use super::*;

#[test]
fn a_dropped_scenario_removes_its_directory_with_a_locked_directory_inside_and_without() {
    for item in [Item::Locked, Item::Searchable, Item::File("{}")] {
        let scenario = Scenario::new();
        scenario.write("held/x", item);
        let root = scenario.workdir().to_path_buf();
        drop(scenario);
        assert!(!root.exists(), "{item:?}");
    }
}

#[test]
fn a_dropped_scenario_removes_a_locked_directory_inside_a_locked_directory() {
    let scenario = Scenario::new();
    scenario
        .write("held/inner", Item::Locked)
        .write("held", Item::Locked);
    let root = scenario.workdir().to_path_buf();
    drop(scenario);
    assert!(!root.exists());
}

#[test]
fn a_searchable_directory_can_be_passed_through_but_not_listed() {
    let scenario = Scenario::new();
    scenario
        .write("held/x", Item::File("{}"))
        .write("held", Item::Searchable);
    let held = scenario.workdir().join("held");
    assert_eq!(fs::read_to_string(held.join("x")).unwrap(), "{}");
    // A superuser lists any directory.
    if fs::read_dir(&held).is_ok() {
        return;
    }
    let mode = fs::metadata(&held).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o111);
}

#[test]
fn a_fed_fifo_reads_as_empty_and_feeding_returns_when_nothing_reads_it() {
    let scenario = Scenario::new();
    scenario.write("fifo", Item::Fifo);
    let fifo = scenario.workdir().join("fifo");
    let read = scenario.feeding("fifo", || std::fs::read(&fifo).unwrap());
    assert_eq!(read, b"");
    assert_eq!(scenario.feeding("fifo", || 7), 7);
}

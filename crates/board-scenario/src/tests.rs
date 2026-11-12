//! The scenario builder cleans up after itself.

use super::*;

#[test]
fn a_dropped_scenario_removes_its_directory_with_a_locked_directory_inside_and_without() {
    for item in [
        Item::Locked,
        Item::Searchable,
        Item::ReadOnly,
        Item::File("{}"),
    ] {
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

#[test]
fn a_locked_searchable_or_read_only_directory_has_its_own_mode() {
    for (item, mode) in [
        (Item::Locked, 0o000),
        (Item::Searchable, 0o111),
        (Item::ReadOnly, 0o555),
    ] {
        let scenario = Scenario::new();
        scenario.write("held", item);
        let held = fs::metadata(scenario.workdir().join("held")).unwrap();
        assert_eq!(held.permissions().mode() & 0o777, mode, "{item:?}");
    }
}

#[test]
fn a_board_holds_the_bare_config_and_nothing_else_in_it() {
    let scenario = Scenario::board();
    let bytes = fs::read(scenario.workdir().join(".board/board.json")).unwrap();
    let classified = board_model::classify(".board/board.json", Some(&bytes));
    assert_eq!(classified.code, None);
    let board_model::Content::Record { record_type, value } = classified.content else {
        panic!("the config is no record: {classified:?}");
    };
    assert_eq!(record_type.as_str(), board_model::CONFIG_TYPE);
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
}

/// The repository's own configuration, without the machine's, as CI reads it.
fn isolated(scenario: &Scenario) -> gix::Repository {
    gix::open_opts(scenario.workdir(), gix::open::Options::isolated()).unwrap()
}

#[test]
fn an_identified_repository_commits_as_board_test_and_its_twin_has_nobody_to_commit_as() {
    let scenario = Scenario::new();
    assert!(isolated(&scenario).committer().is_none());
    let git = isolated(scenario.identified());
    let tree = gix::ObjectId::empty_tree(git.object_hash());
    let none: [gix::ObjectId; 0] = [];
    let commit = git.commit("HEAD", "first", tree, none).unwrap();
    let commit = git.find_commit(commit).unwrap();
    let author = commit.author().unwrap();
    assert_eq!(author.name, "Board Test");
    assert_eq!(author.email, "board@example.com");
    assert_eq!(scenario.head().unwrap().id, model(commit.id));
}

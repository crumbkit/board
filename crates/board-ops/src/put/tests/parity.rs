//! `put` refuses a record with the code `check` gives the same bytes.

use board_model::Code;
use board_scenario::{Item, Scenario};
use board_tree::Status;
use proptest::prelude::*;

use super::{board, put};
use crate::put::Expect;
use crate::tests::verdicts::{Case, TASK_SCHEMA_PATH, agrees, case, records, schemas, tasks};
use crate::tests::{TASK_SCHEMA, open, task, unreadable};

/// What `check` and then `put` say of each record, written at
/// `tasks/r{n}.json` of `scenario`, a test board with whatever is at the
/// task schema's path.
fn verdicts(scenario: &Scenario, records: &[String]) -> Vec<(Option<Code>, Result<(), Code>)> {
    let physical = |n: usize| format!("tasks/r{n}.json");
    for (n, record) in records.iter().enumerate() {
        scenario.write(&physical(n), Item::File(record));
    }
    let report = open(scenario).check(None, None).unwrap();
    let checked: Vec<Option<Code>> = (0..records.len())
        .map(|n| {
            let entry = report
                .entries
                .iter()
                .find(|entry| entry.path == physical(n))
                .unwrap();
            match entry.status {
                Status::Ok => None,
                Status::Degraded(code) => Some(code),
            }
        })
        .collect();
    for n in 0..records.len() {
        scenario.write(&physical(n), Item::Deleted);
    }
    records
        .iter()
        .zip(checked)
        .enumerate()
        .map(|(n, (record, checked))| {
            let put = put(scenario, &format!("/tasks/r{n}"), Expect::Absent, record).map(|_| ());
            (checked, put)
        })
        .collect()
}

/// A test board with a file holding `schema` at the task schema's path, or
/// no file.
fn slot(schema: Option<&str>) -> Scenario {
    let scenario = board();
    scenario.write(TASK_SCHEMA_PATH, schema.map_or(Item::Deleted, Item::File));
    scenario
}

/// Asserts that `check` and `put` both give `code` for a task record of
/// `scenario`.
fn agree_with(scenario: &Scenario, code: Code, case: &str) {
    let verdicts = verdicts(scenario, &[task("")]);
    assert_eq!(verdicts, [(Some(code), Err(code))], "{case}");
}

#[test]
fn put_agrees_with_check_when_the_schema_cannot_be_read() {
    let scenario = board();
    if !unreadable(&scenario, TASK_SCHEMA_PATH, TASK_SCHEMA) {
        return;
    }
    agree_with(&scenario, Code::RepoUnreadable, "unreadable");
}

#[test]
fn put_agrees_with_check_when_a_directory_is_at_the_schema_path_and_when_it_is_a_submodule() {
    let directory = slot(None);
    directory.write(
        &format!("{TASK_SCHEMA_PATH}/held.json"),
        Item::File(TASK_SCHEMA),
    );
    agree_with(&directory, Code::RecordUnknownType, "a directory");

    let nested = slot(None);
    nested.write(TASK_SCHEMA_PATH, Item::Submodule);
    agree_with(&nested, Code::RepoUnreadable, "a nested repository");

    let linked = slot(None);
    let commit = linked.commit_items(&[(TASK_SCHEMA_PATH.as_bytes(), Item::Submodule)]);
    let held = format!("{TASK_SCHEMA_PATH}/x");
    linked
        .track(commit)
        .write(&held, Item::File(""))
        .write(&held, Item::Deleted);
    agree_with(&linked, Code::RepoUnreadable, "a gitlink");

    let beneath = board();
    beneath.write(".board/types/lr", Item::Submodule);
    agree_with(&beneath, Code::RepoUnreadable, "beneath a submodule");
}

#[test]
fn put_agrees_with_check_when_a_symlink_is_above_the_schema() {
    let scenario = board();
    scenario
        .write("elsewhere/task/v1.schema.json", Item::File(TASK_SCHEMA))
        .write(".board/types/lr", Item::Deleted)
        .write(".board/types/lr", Item::Symlink("../../elsewhere"));
    agree_with(&scenario, Code::RecordUnknownType, "a symlink above");
}

#[test]
fn put_agrees_with_check_when_a_directory_at_or_above_the_schema_cannot_be_listed() {
    // A superuser lists any directory, so the case cannot be made as root.
    let locked = |scenario: &Scenario, path: &str| {
        scenario.write(path, Item::Locked);
        std::fs::read_dir(scenario.workdir().join(path)).is_err()
    };
    let at = slot(None);
    if !locked(&at, TASK_SCHEMA_PATH) {
        return;
    }
    agree_with(
        &at,
        Code::RepoUnreadable,
        "a locked directory at the schema path",
    );

    let above = board();
    locked(&above, ".board/types/lr");
    agree_with(&above, Code::RepoUnreadable, "a locked directory above");
}

#[test]
fn put_agrees_with_check_on_every_listed_record_under_every_schema() {
    let records = records();
    for schema in schemas() {
        let verdicts = verdicts(&slot(schema.as_deref()), &records);
        for (record, (checked, put)) in records.iter().zip(&verdicts) {
            assert!(
                agrees(record.as_bytes(), *checked, *put),
                "{record:?} under {schema:?}: check gives {checked:?}, put gives {put:?}"
            );
        }
        if schema.as_deref() == Some(TASK_SCHEMA) {
            for (record, expected) in tasks() {
                let at = records.iter().position(|listed| *listed == record).unwrap();
                assert_eq!(verdicts[at].0, expected, "{record}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn put_refuses_with_the_code_check_gives_the_same_bytes_and_admits_the_rest(
        Case { schema, records } in case(),
    ) {
        let verdicts = verdicts(&slot(schema.as_deref()), &records);
        for (record, (checked, put)) in records.iter().zip(verdicts) {
            prop_assert!(
                agrees(record.as_bytes(), checked, put),
                "{:?}: check gives {:?}, put gives {:?}",
                record,
                checked,
                put
            );
        }
    }
}

//! `put` refuses a record with the code `check` gives the same bytes.

use board_model::Code;
use board_scenario::{Item, Scenario};
use board_tree::Status;
use proptest::prelude::*;

use super::{board, put};
use crate::put::Expect;
use crate::tests::verdicts::{Case, TASK_SCHEMA_PATH, agrees, case, records, schemas, tasks};
use crate::tests::{TASK_SCHEMA, locked, open, task, unreadable};

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

/// Asserts that `check` and `put` both admit a task record of `scenario`.
fn admitted(scenario: &Scenario, case: &str) {
    let verdicts = verdicts(scenario, &[task("")]);
    assert_eq!(verdicts, [(None, Ok(()))], "{case}");
}

#[test]
fn put_agrees_with_check_when_the_schema_cannot_be_read() {
    admitted(&board(), "a readable schema");
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
fn put_agrees_with_check_that_a_schema_symlinked_to_a_file_is_read_and_one_leaving_the_repository_is_not()
 {
    let linked = |text: &str| {
        let scenario = slot(None);
        scenario
            .write("elsewhere/task.json", Item::File(TASK_SCHEMA))
            .write(TASK_SCHEMA_PATH, Item::Symlink(text));
        scenario
    };
    admitted(
        &linked("../../../../elsewhere/task.json"),
        "a symlink to a file",
    );
    agree_with(
        &linked("../../../../../elsewhere/task.json"),
        Code::SymlinkOutside,
        "a symlink leaving the repository",
    );
}

#[test]
fn put_agrees_with_check_when_a_directory_at_or_above_the_schema_cannot_be_listed() {
    admitted(&board(), "every directory can be listed");
    let at = slot(None);
    if !locked(&at, TASK_SCHEMA_PATH, Item::Locked) {
        return;
    }
    agree_with(
        &at,
        Code::RepoUnreadable,
        "a locked directory at the schema path",
    );

    let above = board();
    locked(&above, ".board/types/lr", Item::Locked);
    agree_with(&above, Code::RepoUnreadable, "a locked directory above");

    let searchable = board();
    locked(&searchable, ".board/types/lr", Item::Searchable);
    agree_with(
        &searchable,
        Code::RepoUnreadable,
        "a directory above that can be searched but not listed",
    );
}

#[test]
fn put_agrees_with_check_when_the_schema_or_a_directory_above_it_is_ignored_and_admits_when_it_is_tracked()
 {
    let ignore = format!("{TASK_SCHEMA_PATH}\n");
    let ignored = board();
    ignored.write(".gitignore", Item::File(&ignore));
    agree_with(&ignored, Code::RecordUnknownType, "an ignored schema");

    let above = board();
    above.write(".gitignore", Item::File(".board/types/lr/task/\n"));
    agree_with(
        &above,
        Code::RecordUnknownType,
        "an ignored directory above",
    );

    let tracked = board();
    let commit = tracked.commit(&[(TASK_SCHEMA_PATH, TASK_SCHEMA)]);
    tracked
        .track(commit)
        .write(".gitignore", Item::File(&ignore));
    admitted(&tracked, "a tracked schema that an ignore pattern matches");

    let tracked_above = board();
    let commit = tracked_above.commit(&[(TASK_SCHEMA_PATH, TASK_SCHEMA)]);
    tracked_above
        .track(commit)
        .write(".gitignore", Item::File(".board/types/lr/task/\n"));
    admitted(
        &tracked_above,
        "a tracked schema beneath a directory an ignore pattern matches",
    );
}

#[test]
fn put_agrees_with_check_when_a_name_on_the_schema_path_is_spelled_in_another_case() {
    admitted(&board(), "every name spelled as the record type spells it");
    // A case-insensitive file system, such as macOS's, opens these paths
    // under the record type's spelling; the walk lists them as written.
    for misspelt in [
        ".board/types/lr/TASK/v1.schema.json",
        ".board/types/lr/task/V1.schema.json",
    ] {
        let scenario = board();
        scenario
            .write(".board/types/lr/task", Item::Deleted)
            .write(misspelt, Item::File(TASK_SCHEMA));
        agree_with(&scenario, Code::RecordUnknownType, misspelt);
    }
}

#[test]
fn put_agrees_with_check_when_a_fifo_is_at_the_schema_path() {
    admitted(&board(), "a readable schema");
    // The walk lists only files and symlinks. A writer closes the FIFO at
    // once, so a put that opens it reads an empty schema instead of waiting.
    let fifo = slot(None);
    fifo.write(TASK_SCHEMA_PATH, Item::Fifo);
    let verdicts = fifo.feeding(TASK_SCHEMA_PATH, || verdicts(&fifo, &[task("")]));
    let unknown = Code::RecordUnknownType;
    assert_eq!(verdicts, [(Some(unknown), Err(unknown))], "a FIFO");
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

#[test]
fn put_names_the_text_of_a_schema_symlink_that_dangles_and_not_of_one_to_a_file_that_cannot_be_read()
 {
    let text = "../../../../elsewhere/task.json";
    let scenario = slot(None);
    scenario.write(TASK_SCHEMA_PATH, Item::Symlink(text));
    let refused = |code, target| {
        agree_with(
            &scenario,
            code,
            "a schema symlink to no file or to one unread",
        );
        let refused = open(&scenario)
            .put(
                &super::path("/tasks/r"),
                Expect::Absent,
                task("").as_bytes(),
            )
            .unwrap_err();
        let fields = refused.fields();
        assert_eq!((refused.code(), fields.target.as_deref()), (code, target));
    };
    refused(Code::SymlinkDangling, Some(text));
    scenario.write("elsewhere/task.json", Item::File(TASK_SCHEMA));
    admitted(&scenario, "a schema symlink to a file");
    if unreadable(&scenario, "elsewhere/task.json", TASK_SCHEMA) {
        refused(Code::RepoUnreadable, None);
    }
}

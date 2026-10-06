//! `put` refuses a record with the code `check` gives the same bytes.

use board_model::Code;
use board_scenario::Item;
use board_tree::Status;
use proptest::prelude::*;

use super::{board, put};
use crate::put::Expect;
use crate::tests::verdicts::{Case, TASK_SCHEMA_PATH, agrees, case, records, schemas, tasks};
use crate::tests::{TASK_SCHEMA, open};

/// What `check` and then `put` say of each record, written at
/// `tasks/r{n}.json` of a test board with `schema` at the task schema's path.
fn verdicts(schema: Option<&str>, records: &[String]) -> Vec<(Option<Code>, Result<(), Code>)> {
    let scenario = board();
    scenario.write(TASK_SCHEMA_PATH, schema.map_or(Item::Deleted, Item::File));
    let physical = |n: usize| format!("tasks/r{n}.json");
    for (n, record) in records.iter().enumerate() {
        scenario.write(&physical(n), Item::File(record));
    }
    let report = open(&scenario).check(None, None).unwrap();
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
            let put = put(&scenario, &format!("/tasks/r{n}"), Expect::Absent, record).map(|_| ());
            (checked, put)
        })
        .collect()
}

#[test]
fn put_agrees_with_check_on_every_listed_record_under_every_schema() {
    let records = records();
    for schema in schemas() {
        let verdicts = verdicts(schema.as_deref(), &records);
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
        let verdicts = verdicts(schema.as_deref(), &records);
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

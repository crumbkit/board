//! A put that waits for the lock checks its record, and finds where it
//! lands, against the board as it is once it holds the lock.

use std::io::{BufRead, Read};
use std::sync::mpsc;

use board_model::{Code, Lined};
use board_scenario::{Item, Scenario};
use board_tree::LOCK_WAIT;

use super::{board, canonical, file};
use crate::put::{PutError, PutReport};
use crate::tests::{open, task};

/// Batch input that sends on `done` when the batch reads past its end.
struct Signalling<'a> {
    bytes: &'a [u8],
    done: Option<mpsc::Sender<()>>,
}

impl Read for Signalling<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl BufRead for Signalling<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.bytes.is_empty()
            && let Some(done) = self.done.take()
        {
            let _ = done.send(());
        }
        Ok(self.bytes)
    }

    fn consume(&mut self, count: usize) {
        self.bytes = &self.bytes[count..];
    }
}

/// Runs a batch of `input` while the test holds the lock. Once the batch
/// has read all of its input, `meanwhile` changes the board, and then the
/// lock is released. A batch reads its whole input before it takes the
/// lock, so the batch is either waiting for the lock or about to take it
/// when `meanwhile` runs, and it cannot write before the lock is released.
fn batch_while_locked(
    scenario: &Scenario,
    input: &str,
    meanwhile: impl FnOnce(),
) -> Result<PutReport, PutError> {
    let held = open(scenario).repository().lock(LOCK_WAIT).unwrap();
    let root = scenario.workdir().to_path_buf();
    let (done, read) = mpsc::channel();
    std::thread::scope(|scope| {
        let put = scope.spawn(move || {
            crate::Board::open(&root, None)
                .unwrap()
                .put_batch(&mut Signalling {
                    bytes: input.as_bytes(),
                    done: Some(done),
                })
        });
        read.recv().unwrap();
        meanwhile();
        drop(held);
        put.join().unwrap()
    })
}

fn line(path: &str, record: &str) -> String {
    format!(r#"{{"path": "{path}", "record": {record}}}"#) + "\n"
}

/// The refusals of a refused batch, as (line, code).
fn refused(result: Result<PutReport, PutError>) -> (Code, Vec<(usize, Code)>) {
    match result.unwrap_err() {
        PutError::Batch { code, refusals } => (
            code,
            refusals
                .iter()
                .map(|Lined { line, code, .. }| (*line, *code))
                .collect(),
        ),
        other => panic!("not a batch refusal: {other}"),
    }
}

const SCHEMA: &str = ".board/types/lr/task/v1.schema.json";

#[test]
fn a_schema_changed_while_a_put_waits_for_the_lock_to_refuse_its_record_refuses_the_put() {
    let scenario = board();
    let result = batch_while_locked(&scenario, &line("/tasks/a", &task("")), || {
        scenario.write(
            SCHEMA,
            Item::File(r#"{"type": "object", "required": ["title", "owner"]}"#),
        );
    });
    assert_eq!(
        refused(result),
        (Code::PutInvalid, vec![(1, Code::RecordInvalid)])
    );
    assert_eq!(file(&scenario, "tasks/a.json"), None);
}

#[test]
fn a_schema_changed_while_a_put_waits_for_the_lock_to_still_admit_its_record_admits_the_put() {
    let scenario = board();
    let result = batch_while_locked(&scenario, &line("/tasks/a", &task("")), || {
        scenario.write(
            SCHEMA,
            Item::File(r#"{"type": "object", "required": ["title"], "properties": {"owner": {"type": "string"}}}"#),
        );
    });
    assert_eq!(result.unwrap().writes.len(), 1);
    assert_eq!(file(&scenario, "tasks/a.json"), Some(canonical(&task(""))));
}

/// A board with `/links/sub` linking to `/one`, and `/two` beside it.
fn linked() -> Scenario {
    let scenario = board();
    scenario
        .write("one/x.json", Item::File(&task("")))
        .write("two/x.json", Item::File(&task("")))
        .write(
            "links/sub.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/one"}"#),
        );
    scenario
}

#[test]
fn a_link_retargeted_while_a_put_waits_for_the_lock_to_a_missing_directory_refuses_the_put() {
    let scenario = linked();
    let result = batch_while_locked(&scenario, &line("/links/sub/a", &task("")), || {
        scenario.write(
            "links/sub.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/gone"}"#),
        );
    });
    assert_eq!(
        refused(result),
        (Code::PutInvalid, vec![(1, Code::RefDangling)])
    );
    assert_eq!(file(&scenario, "one/a.json"), None);
    assert_eq!(file(&scenario, "gone/a.json"), None);
}

#[test]
fn a_link_retargeted_while_a_put_waits_for_the_lock_to_an_existing_directory_writes_there() {
    let scenario = linked();
    let result = batch_while_locked(&scenario, &line("/links/sub/a", &task("")), || {
        scenario.write(
            "links/sub.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/two"}"#),
        );
    });
    assert_eq!(result.unwrap().writes[0].location.path, "two/a.json");
    assert_eq!(file(&scenario, "one/a.json"), None);
    assert_eq!(file(&scenario, "two/a.json"), Some(canonical(&task(""))));
}

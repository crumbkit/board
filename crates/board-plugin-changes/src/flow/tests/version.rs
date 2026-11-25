//! The line that writes a flow's version expects nothing at its path, the
//! answer `blob_at` read. Of two starts that both find the version
//! missing, the second is refused as stale and writes nothing, and its
//! rerun finds the version and writes no version line.

use std::cell::RefCell;

use board_model::ObjectId;
use board_ops::{Host, Limit, PutError, PutReport, QueryError, QueryReport, Settings};
use serde_json::{Value, json};

use crate::flow::{development, version_line, version_path};

/// The blob another writer stores at the version.
const THEIRS: &str = "1111111111111111111111111111111111111111";

/// A host that holds at most the version, as `THEIRS`, and applies each
/// put line's expect as Board does under its put lock: a line whose expect
/// the held blob does not match refuses the whole batch as stale.
struct Lacking {
    settings: Settings,
    /// The blob at the version, if any.
    held: RefCell<Option<String>>,
    /// Whether another writer stores the version just before the next put.
    racing: RefCell<bool>,
    /// The batches that landed.
    landed: RefCell<Vec<Vec<Value>>>,
}

impl Lacking {
    fn new(racing: bool) -> Self {
        Self {
            settings: Settings {
                now: std::time::SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: board_ops::Variables::default(),
                cache_home: None,
            },
            held: RefCell::new(None),
            racing: RefCell::new(racing),
            landed: RefCell::default(),
        }
    }
}

impl Host for Lacking {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        let rows = (self.held.borrow().iter())
            .map(|blob| vec![json!(blob)])
            .collect();
        Ok(QueryReport {
            columns: vec!["blob".into()],
            rows,
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        unreachable!("version_line resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        unreachable!("version_line reads no files")
    }

    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        if self.racing.replace(false) {
            *self.held.borrow_mut() = Some(THEIRS.to_owned());
        }
        for line in lines {
            let held = self.held.borrow().clone();
            let matches = match line["expect"].as_str() {
                None => true,
                Some("none") => held.is_none(),
                Some(blob) => held.as_deref() == Some(blob),
            };
            if !matches {
                return Err(PutError::Stale {
                    path: line["path"].as_str().unwrap().parse().unwrap(),
                    found: held.as_deref().and_then(ObjectId::from_hex),
                });
            }
        }
        *self.held.borrow_mut() = Some("2222222222222222222222222222222222222222".to_owned());
        self.landed.borrow_mut().push(lines.to_vec());
        Ok(PutReport {
            writes: Vec::new(),
            removed: Vec::new(),
            absent: Vec::new(),
            commit: None,
        })
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        unreachable!("version_line calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        unreachable!("version_line opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        unreachable!("version_line asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("version_line syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("version_line updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[test]
fn the_line_that_writes_a_missing_version_expects_nothing_there() {
    let host = Lacking::new(false);
    let flow = development();
    let (at, line) = version_line(&host, &flow).unwrap();
    assert_eq!(at, version_path(&flow));
    let line = line.expect("a board that lacks the version gets a line");
    assert_eq!(line["path"], at.as_str());
    assert_eq!(line["expect"], "none", "{line}");
    assert_eq!(line["record"], serde_json::to_value(&flow).unwrap());
    crate::board::put(&host, &[line]).unwrap();
    assert_eq!(
        host.landed.borrow().len(),
        1,
        "with no other writer it lands"
    );
}

#[test]
fn a_version_another_writer_stores_first_makes_the_put_stale_and_the_rerun_writes_no_line() {
    let host = Lacking::new(true);
    let flow = development();
    let (_, line) = version_line(&host, &flow).unwrap();
    let refused = crate::board::put(&host, &[line.unwrap()]).unwrap_err();
    assert_eq!(
        refused.fields["code"], "board::put::stale",
        "{}",
        refused.message
    );
    assert!(host.landed.borrow().is_empty(), "nothing is written");
    assert_eq!(
        host.held.borrow().as_deref(),
        Some(THEIRS),
        "theirs is kept"
    );
    let (_, again) = version_line(&host, &flow).unwrap();
    assert_eq!(again, None, "the rerun finds the version");
}

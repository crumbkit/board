//! The line that writes a flow's version takes no expect, so two starts
//! that both find the version missing both succeed: the version's path
//! names its content, and the second writes the same bytes the first did.

use board_ops::{Host, Limit, QueryError, QueryReport, Settings};

use crate::flow::{development, version_line, version_path};

/// A host that holds nothing: every query answers with no rows.
struct Lacking(Settings);

impl Host for Lacking {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        Ok(QueryReport {
            columns: vec!["blob".into()],
            rows: Vec::new(),
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

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        unreachable!("version_line writes nothing")
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
        &self.0
    }
}

#[test]
fn the_line_that_writes_a_missing_version_takes_no_expect() {
    let host = Lacking(Settings {
        now: std::time::SystemTime::UNIX_EPOCH,
        day_start: None,
        utc_offset: 0,
        caller: None,
        environment: board_ops::Variables::default(),
        cache_home: None,
    });
    let flow = development();
    let (at, line) = version_line(&host, &flow).unwrap();
    assert_eq!(at, version_path(&flow));
    let line = line.expect("a board that lacks the version gets a line");
    assert_eq!(line["path"], at.as_str());
    assert_eq!(line.get("expect"), None, "{line}");
    assert_eq!(line["record"], serde_json::to_value(&flow).unwrap());
}

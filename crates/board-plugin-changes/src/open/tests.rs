//! `open` reads each flow path once per call, however many changes name it,
//! and lists every change as reading each change's flow on its own would.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, Refusal};
use board_ops::{
    Absent, DecideReport, Elsewhere, GetReport, Limit, MountError, MountUpdateReport, PutError,
    PutReport, QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings, Shown,
    SyncError, Synced, Variables,
};
use board_plugin::shape::Record;
use board_plugin::{CallReport, Target};
use serde_json::{Value, json};

use super::*;
use crate::change::parsed;
use crate::flow::{Flow, development, of_change};

/// A host holding changes and the flows they name, which counts queries.
struct Holding {
    /// Each change's path and record.
    changes: Vec<(String, Value)>,
    /// Each flow's path and record.
    flows: BTreeMap<String, Value>,
    queries: Cell<usize>,
    settings: Settings,
}

impl Holding {
    /// `count` changes, the `n`th naming the flow at `/flows/f<n % paths>`,
    /// where each but the last of `paths` flows is held.
    fn new(count: usize, paths: usize) -> Self {
        let flow_path = |n: usize| format!("/flows/f{}", n % paths);
        let changes = (0..count)
            .map(|n| {
                let record = json!({
                    "$type": crate::CHANGE_TYPE,
                    "title": format!("change {n}"),
                    "mount": "/repos/board",
                    "flow": flow_path(n),
                    "phase": "build",
                });
                (format!("/changes/c{n:02}"), record)
            })
            .collect();
        let flows = (0..paths - 1)
            .map(|n| {
                let mut flow = serde_json::to_value(development()).unwrap();
                flow["name"] = json!(format!("flow-{n}"));
                (flow_path(n), flow)
            })
            .collect();
        Self {
            changes,
            flows,
            queries: Cell::new(0),
            settings: Settings {
                now: SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables::default(),
                cache_home: None,
            },
        }
    }

    /// What `open` lists with no input, and how many queries it made.
    fn opened(&self) -> (Value, usize) {
        self.queries.set(0);
        let output = open(self, &json!({})).unwrap();
        (output, self.queries.get())
    }
}

fn report(rows: Vec<Vec<Value>>) -> QueryReport {
    QueryReport {
        columns: Vec::new(),
        rows,
        truncated: false,
        over_budget: false,
        memory: None,
    }
}

impl Host for Holding {
    /// Answers the query for open changes, for done changes and for
    /// prototypes, which there are none of, and for the flow at one path.
    fn query(&self, sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        self.queries.set(self.queries.get() + 1);
        // No change is done, so none owes a check.
        if sql.contains("= 'done'") {
            return Ok(report(Vec::new()));
        }
        if sql.starts_with("SELECT logical, code, body FROM records") {
            let rows = (self.changes.iter())
                .map(|(path, record)| vec![json!(path), Value::Null, json!(record.to_string())])
                .collect();
            return Ok(report(rows));
        }
        if sql.starts_with("SELECT e.type, e.code, b.body FROM entries") {
            let path = (sql.rsplit_once("= '"))
                .map(|(_, rest)| rest.trim_end_matches('\''))
                .unwrap();
            let rows = (self.flows.get(path))
                .map(|flow| vec![json!(Flow::TYPE), Value::Null, json!(flow.to_string())])
                .into_iter()
                .collect();
            return Ok(report(rows));
        }
        assert!(sql.contains("/prototype"), "{sql}");
        Ok(report(Vec::new()))
    }

    /// Resolves nothing, so `open` finds no sync recorded.
    fn refs(
        &self,
        _paths: &[LogicalPath],
        _absent: Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        Ok(Vec::new())
    }

    fn get_each(
        &self,
        _paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        unreachable!("open reads no files")
    }

    fn put(&self, _lines: &[Value]) -> Result<PutReport, PutError> {
        unreachable!("open writes nothing")
    }

    fn call(&self, _target: &Target, _input: &Value) -> Result<CallReport, Box<Refusal>> {
        unreachable!("open calls no plugin")
    }

    fn board(&self, _elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("open opens no other board")
    }

    fn decide(
        &self,
        _capability: &str,
        _path: &LogicalPath,
        _shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        unreachable!("open asks the owner nothing")
    }

    fn sync(&self, _push: bool, _deadline: Duration) -> Result<Synced, SyncError> {
        unreachable!("open syncs nothing")
    }

    fn mount_update(
        &self,
        _path: &LogicalPath,
        _deadline: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        unreachable!("open updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[test]
fn open_reads_each_flow_path_once_however_many_changes_name_it() {
    let (few, few_queries) = Holding::new(3, 2).opened();
    let (many, many_queries) = Holding::new(9, 2).opened();
    assert_eq!(few["total"], 3);
    assert_eq!(many["total"], 9);
    assert_eq!(few_queries, many_queries);
}

#[test]
fn open_lists_each_changes_flow_as_reading_it_on_its_own_does() {
    let host = Holding::new(9, 3);
    let (output, _) = host.opened();
    let expected: Vec<Value> = (host.changes.iter())
        .map(|(path, record)| {
            let change = parsed(Some(&json!(record.to_string()))).unwrap();
            let flow = (of_change(&host, path, &change).ok().flatten())
                .map(|flow| FlowSummary::from(&flow));
            serde_json::to_value(listed(path, change, flow, None)).unwrap()
        })
        .collect();
    assert_eq!(output["changes"], json!(expected));
    // A third of the changes name the flow the board does not hold.
    let unflowed = (output["changes"].as_array().unwrap().iter())
        .filter(|listed| listed.get("flow").is_none())
        .count();
    assert_eq!(unflowed, 3);
}

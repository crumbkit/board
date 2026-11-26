//! `owing` judges each done change from its flow and its plan's answers
//! before it observes the change, so a board of done changes that owe
//! nothing costs the same few queries however many changes it holds.

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
use crate::flow::{Flow, development, shipped};
use crate::records::PlanRecord;

/// The flow whose last phase has a check that does not block: the shipped
/// development flow, whose rechecks apply when the plan stores data.
const RECHECKED: &str = "/flows/development/v";
/// The flow whose last phase has only checks that block.
const BLOCKING: &str = "/flows/small/v";

/// A host holding done changes, the flows they name and their plans, which
/// counts queries and answers every other query with no rows.
struct Holding {
    /// Each done change's path and record.
    changes: Vec<(String, Value)>,
    flows: BTreeMap<String, Value>,
    /// Each plan record by path.
    plans: BTreeMap<String, Value>,
    queries: Cell<usize>,
    settings: Settings,
}

impl Holding {
    /// `count` done changes, alternately on [`BLOCKING`] and on
    /// [`RECHECKED`], each with a plan whose answers store no data.
    fn new(count: usize) -> Self {
        let mut changes = Vec::new();
        let mut plans = BTreeMap::new();
        for n in 0..count {
            let path = format!("/changes/c{n:02}");
            let plan = format!("{path}/plans/1");
            let flow = if n % 2 == 0 { BLOCKING } else { RECHECKED };
            changes.push((
                path,
                json!({
                    "$type": crate::CHANGE_TYPE,
                    "title": format!("change {n}"),
                    "mount": "/repos/board",
                    "flow": flow,
                    "phase": "done",
                    "touched": "1970-01-01",
                    "plan": plan,
                }),
            ));
            plans.insert(
                plan,
                json!({
                    "$type": PlanRecord::TYPE,
                    "text": format!("/changes/c{n:02}/plans/1.md"),
                    "uses": [],
                    "answers": {"ux": "none", "data": "none", "lifecycle": "none"},
                }),
            );
        }
        let flows = BTreeMap::from([
            (
                RECHECKED.to_owned(),
                serde_json::to_value(development()).unwrap(),
            ),
            (
                BLOCKING.to_owned(),
                serde_json::to_value(shipped("small").unwrap()).unwrap(),
            ),
        ]);
        Self {
            changes,
            flows,
            plans,
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

    /// What `owing` finds, and how many queries it made.
    fn owing(&self) -> (Vec<Owed>, usize) {
        self.queries.set(0);
        let (found, _) = owing(self).unwrap();
        (found, self.queries.get())
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
    /// Answers the query for done changes, for the flows and for the plans
    /// it names; any other query finds nothing.
    fn query(&self, sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        self.queries.set(self.queries.get() + 1);
        if sql.contains("= 'done'") {
            let rows = (self.changes.iter())
                .map(|(path, record)| vec![json!(path), Value::Null, json!(record.to_string())])
                .collect();
            return Ok(report(rows));
        }
        let named = |path: &str| sql.contains(&format!("'{path}'"));
        if sql.starts_with("SELECT e.logical, e.type, e.code, b.body FROM entries") {
            let rows = (self.flows.iter())
                .filter(|(path, _)| named(path))
                .map(|(path, flow)| {
                    vec![
                        json!(path),
                        json!(Flow::TYPE),
                        Value::Null,
                        json!(flow.to_string()),
                    ]
                })
                .collect();
            return Ok(report(rows));
        }
        if sql.contains(PlanRecord::TYPE) {
            let rows = (self.plans.iter())
                .filter(|(path, _)| named(path))
                .map(|(path, plan)| vec![json!(path), json!(plan.to_string())])
                .collect();
            return Ok(report(rows));
        }
        Ok(report(Vec::new()))
    }

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
        Ok(Vec::new())
    }

    fn put(&self, _lines: &[Value]) -> Result<PutReport, PutError> {
        unreachable!("owing writes nothing")
    }

    fn call(&self, _target: &Target, _input: &Value) -> Result<CallReport, Box<Refusal>> {
        unreachable!("owing calls no plugin")
    }

    fn board(&self, _elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("owing opens no other board")
    }

    fn decide(
        &self,
        _capability: &str,
        _path: &LogicalPath,
        _shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        unreachable!("owing asks the owner nothing")
    }

    fn sync(&self, _push: bool, _deadline: Duration) -> Result<Synced, SyncError> {
        unreachable!("owing syncs nothing")
    }

    fn mount_update(
        &self,
        _path: &LogicalPath,
        _deadline: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        unreachable!("owing updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[test]
fn done_changes_that_owe_nothing_cost_the_same_queries_however_many_there_are() {
    let (few, few_queries) = Holding::new(4).owing();
    let (many, many_queries) = Holding::new(40).owing();
    assert!(few.is_empty() && many.is_empty());
    assert_eq!(few_queries, many_queries);
    // The list of done changes, their flows, and their plans.
    assert_eq!(many_queries, 3);
}

#[test]
fn a_done_change_whose_plan_stores_data_is_observed_and_owes_its_rechecks() {
    let mut host = Holding::new(4);
    host.plans.get_mut("/changes/c01/plans/1").unwrap()["answers"]["data"] = json!("stored");
    let (found, _) = host.owing();
    let paths: Vec<&str> = found.iter().map(|owed| owed.path.as_str()).collect();
    assert_eq!(paths, ["/changes/c01"]);
}

#[test]
fn done_changes_left_to_observe_read_their_attached_records_together() {
    let stored = |count: usize| {
        let mut host = Holding::new(count);
        for plan in host.plans.values_mut() {
            plan["answers"]["data"] = json!("stored");
        }
        host
    };
    let (few, few_queries) = stored(4).owing();
    let (many, many_queries) = stored(40).owing();
    assert_eq!((few.len(), many.len()), (2, 20));
    assert_eq!(few_queries, many_queries);
}

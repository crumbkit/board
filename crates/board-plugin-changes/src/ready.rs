//! The `ready` capability: the changes that are ready to land and wait
//! only on the owner, and the done changes that still owe the owner a
//! recheck, each with the kind of check it waits on and why it is failing.
//!
//! A change is ready when it stands in its flow's last phase, every missing
//! check of that phase that blocks is the owner's, and the owner has
//! something to do: a missing check of the owner's, or an open question
//! whose `blocks` names the change.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use board_ops::{Host, PluginError};
use board_repos::Date;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{readable, rows, text};
use crate::change::{attached, change_types, parsed, referenced_in_walk, today};
use crate::diff::{conflicts, mounted};
use crate::flow::{self, Actor, Flows, Kind as CheckKind, of_change};
use crate::gates::{
    Check, REPORT_TYPE, RUN_TYPE, Status, gate, objects, ran_on, reviewed_head, same_commit,
};
use crate::owed::{Owed, owing};
use crate::phases::observe_in;
use crate::records::{ChangeRecord, is_final};
use crate::{PLUGIN, Warning, to_value};

/// The default `limit` (STANDARDS 23).
const READY_LIMIT: usize = 100;
/// How long the merge check of one change may take.
const MERGE_DEADLINE: Duration = Duration::from_secs(5);

/// The changes ready to land.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ReadyInput")]
pub(crate) struct Input {
    #[serde(default = "ready_limit")]
    #[schemars(
        range(min = 1, max = 10_000),
        description = "The most changes to return, 1 to 10,000; 100 when left out."
    )]
    limit: usize,
    #[serde(default = "reads_failing")]
    #[schemars(
        description = "Whether to read why each change is failing; true when left out. False \
                       leaves each `failing` null and runs no merge check."
    )]
    failing: bool,
    #[schemars(
        extend("items" = {"type": "string", "pattern": "^/", "format": "board-path"}),
        description = "Only the changes these open questions block, by the questions' logical \
                       paths; every change when left out."
    )]
    questions: Option<Vec<String>>,
}

fn ready_limit() -> usize {
    READY_LIMIT
}

fn reads_failing() -> bool {
    true
}

/// The kind of check a ready change waits on: a recheck of an
/// understanding check, a data sign-off, a deviation from the agreed
/// mockups, or any other choice of the owner's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ReadyKind {
    Recheck,
    DataSignOff,
    Deviation,
    LandChoice,
}

/// Why a ready change is failing: the newest CI run at its head failed,
/// its head conflicts with the branch its mount follows, or its newest
/// review covers another commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failing {
    CiFailed,
    Conflict,
    Stale,
}

/// One change that waits only on the owner.
///
/// `pull` is its newest pull request record and `head` its head commit. `check` names the missing
/// check it waits on, null when only a question holds it, and `waits_on`
/// says what that check or question asks. `questions` lists the open
/// questions whose `blocks` names the change. `days` counts the days since
/// the change was last touched, null when it records no `touched` day.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Ready {
    path: String,
    title: String,
    pull: Option<String>,
    head: Option<String>,
    kind: ReadyKind,
    check: Option<String>,
    waits_on: String,
    questions: Vec<String>,
    days: Option<i64>,
    failing: Option<Failing>,
}

/// The changes ready to land, oldest first, with the failing ones last.
///
/// `total` counts every such change and `truncated` says whether the limit
/// left some out. `unreadable` is how many changes were left out because
/// Board marks them degraded. `warnings` holds `changes::ready::merge_unread`
/// for each change whose merge check git could not answer.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "ReadyOutput")]
pub(crate) struct Output {
    changes: Vec<Ready>,
    total: usize,
    truncated: bool,
    unreadable: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<Warning>,
}

/// An open question that no answer names: its path, what it asks, and the
/// logical path of the work its `blocks` names.
struct Asked {
    path: String,
    question: String,
}

/// The open questions that no answer names, by the logical path of the
/// change their `blocks` names. A question's `blocks` is read from the root
/// of its own board, as the plans plugin reads it (`question::NAMED` and
/// `open::questions` in that crate).
fn questions(host: &dyn Host) -> Result<BTreeMap<String, Vec<Asked>>, PluginError> {
    let own = "'/' || CASE WHEN q.path LIKE '%.json' THEN substr(q.path, 1, length(q.path) - 5) \
               ELSE q.path END";
    let sql = format!(
        "WITH named AS MATERIALIZED (SELECT a.source AS source, \
         CAST(json_extract(a.body, '$.question') AS TEXT) AS target FROM records a \
         WHERE a.type = 'plans/answer/v1') \
         SELECT q.logical, q.code, json_extract(q.body, '$.question'), \
         substr(q.logical, 1, length(q.logical) - length({own})) || json_extract(q.body, '$.blocks') \
         FROM records q WHERE q.type = 'plans/question/v1' \
         AND json_extract(q.body, '$.status') = 'open' AND json_extract(q.body, '$.blocks') IS NOT NULL \
         AND NOT (EXISTS (SELECT 1 FROM named n WHERE n.source = 'worktree' AND n.target = q.logical) \
         OR EXISTS (SELECT 1 FROM named n WHERE n.source = q.source AND n.target = q.logical) \
         OR EXISTS (SELECT 1 FROM named n WHERE n.source = q.source AND n.target = {own})) \
         ORDER BY q.logical"
    );
    let mut found: BTreeMap<String, Vec<Asked>> = BTreeMap::new();
    for row in rows(host, &sql)? {
        let (Some(path), Some(question), Some(work)) =
            (text(row.first()), text(row.get(2)), text(row.get(3)))
        else {
            continue;
        };
        if !readable(row.get(1)) {
            continue;
        }
        (found.entry(work.to_owned()).or_default()).push(Asked {
            path: path.to_owned(),
            question: question.to_owned(),
        });
    }
    Ok(found)
}

/// The kind of the owner's missing `check` of `phase`: a recheck when an
/// understanding check does not block, a data sign-off when it does, a
/// deviation for a mockups check, and a land choice for any other.
fn kind_of(phase: &flow::Phase, check: &Check) -> ReadyKind {
    let kind = (phase.checks.iter())
        .find(|each| each.id == check.check)
        .and_then(|each| each.kind.known().copied());
    match kind {
        Some(CheckKind::Understood) if !check.blocks => ReadyKind::Recheck,
        Some(CheckKind::Understood) => ReadyKind::DataSignOff,
        Some(CheckKind::Mockups) => ReadyKind::Deviation,
        _ => ReadyKind::LandChoice,
    }
}

/// The days from `touched` to today, or none when it is not a day.
fn days_since(host: &dyn Host, touched: Option<&str>) -> Option<i64> {
    let touched = Date::parse(touched?)?;
    Some(today(host).days() - touched.days())
}

/// Why the change at `path` is failing, or none, and a warning when git
/// could not say whether its head merges.
fn failing_of(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
) -> Result<(Option<Failing>, Option<Warning>), PluginError> {
    let Some(head) = change.head.as_deref() else {
        return Ok((None, None));
    };
    let records = referenced_in_walk(host, &attached(host, path)?)?;
    let newest_run = (records.of_type(RUN_TYPE))
        .filter(|(_, body)| ran_on(body, head))
        .max_by_key(|(_, body)| body.get("finished").and_then(Value::as_str).unwrap_or(""));
    let failed = newest_run.is_some_and(|(_, body)| {
        objects(body, "results").any(|result| {
            matches!(
                result.get("outcome").and_then(Value::as_str),
                Some("fail" | "error")
            )
        })
    });
    if failed {
        return Ok((Some(Failing::CiFailed), None));
    }
    let merged = match mounted(host, &change.mount)? {
        Ok(at) => conflicts(host, &at, head, MERGE_DEADLINE),
        Err(why) => Err(why),
    };
    let warning = match merged {
        Ok(true) => return Ok((Some(Failing::Conflict), None)),
        Ok(false) => None,
        Err(why) => Some(Warning {
            code: "changes::ready::merge_unread",
            detail: format!(
                "whether {path} merges cleanly is not known, so it is not marked failing: {why}"
            ),
        }),
    };
    let stale = (records.of_type(REPORT_TYPE))
        .last()
        .is_some_and(|(_, report)| !same_commit(reviewed_head(report).as_ref(), head));
    Ok((stale.then_some(Failing::Stale), warning))
}

/// The row of a change in the last phase of its flow, when it waits only
/// on the owner, and a warning when its merge check could not be read.
fn ready_in_land(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    asked: &[Asked],
    failing: bool,
) -> Result<(Option<Ready>, Option<Warning>), PluginError> {
    let Some(read) = of_change(host, path, change)? else {
        return Ok((None, None));
    };
    let observed = observe_in(host, path, change, Some(read))?;
    let checks = gate(&observed);
    let blocked = (checks.iter()).any(|each| each.blocking() && each.actor != Actor::Owner);
    let owners: Vec<&Check> = (checks.iter())
        .filter(|each| each.status == Status::Missing && each.actor == Actor::Owner)
        .collect();
    if blocked || (owners.is_empty() && asked.is_empty()) {
        return Ok((None, None));
    }
    let phase = (observed.flow.as_ref())
        .and_then(|flow| flow.phase(&change.phase))
        .expect("the change stands in its flow's last phase, which its caller found");
    let first = owners.iter().find(|each| each.blocks).or(owners.first());
    let (kind, check, waits_on) = match first {
        Some(each) => (
            kind_of(phase, each),
            Some(each.check.clone()),
            each.detail.clone(),
        ),
        None => (ReadyKind::LandChoice, None, asked[0].question.clone()),
    };
    let (failing, warning) = if failing {
        failing_of(host, path, change)?
    } else {
        (None, None)
    };
    let row = Ready {
        path: path.to_owned(),
        title: change.title.clone(),
        pull: change.pulls().last().cloned(),
        head: change.head.clone(),
        kind,
        check,
        waits_on,
        questions: asked.iter().map(|each| each.path.clone()).collect(),
        days: days_since(host, change.touched.as_deref()),
        failing,
    };
    Ok((Some(row), warning))
}

/// The row of a done change that owes the owner a check, as a recheck.
fn recheck(host: &dyn Host, owed: Owed, asked: &[Asked]) -> Option<Ready> {
    let first = (owed.checks.into_iter()).find(|each| each.actor == Actor::Owner)?;
    Some(Ready {
        days: days_since(host, owed.touched.as_deref()),
        path: owed.path,
        title: owed.title,
        pull: owed.pull,
        head: owed.head,
        kind: ReadyKind::Recheck,
        check: Some(first.check),
        waits_on: first.detail,
        questions: asked.iter().map(|each| each.path.clone()).collect(),
        failing: None,
    })
}

pub(crate) fn ready(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        limit,
        failing,
        questions: named,
    } = PluginError::read_input(PLUGIN, "ready", input)?;
    let asked = questions(host)?;
    let only: Option<BTreeSet<String>> = named.map(|named| {
        (asked.iter())
            .filter(|(_, each)| each.iter().any(|one| named.contains(&one.path)))
            .map(|(work, _)| work.clone())
            .collect()
    });
    let asked_of = |path: &str| asked.get(path).map_or(&[][..], Vec::as_slice);
    let sql = format!(
        "SELECT logical, code, body FROM records WHERE type IN ({}) ORDER BY logical",
        change_types()
    );
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let mut unreadable = 0;
    let mut flows = Flows::default();
    for row in rows(host, &sql)? {
        let change = (readable(row.get(1))).then(|| parsed(row.get(2))).flatten();
        let (Some(path), Some(change)) = (text(row.first()), change) else {
            unreadable += 1;
            continue;
        };
        if is_final(&change.phase) || only.as_ref().is_some_and(|only| !only.contains(path)) {
            continue;
        }
        let last =
            (flows.of_change(host, path, &change)).and_then(|flow| flow.phases.last().cloned());
        if last.as_deref() != Some(change.phase.as_str()) {
            continue;
        }
        let (row, warning) = ready_in_land(host, path, &change, asked_of(path), failing)?;
        found.extend(row);
        warnings.extend(warning);
    }
    let (owed, _) = owing(host, only.as_ref())?;
    for each in owed {
        let questions = asked_of(&each.path);
        found.extend(recheck(host, each, questions));
    }
    found.sort_by(|a, b| {
        let key = |row: &Ready| {
            (
                row.failing.is_some(),
                std::cmp::Reverse(row.days.unwrap_or(-1)),
            )
        };
        key(a).cmp(&key(b)).then_with(|| a.path.cmp(&b.path))
    });
    let total = found.len();
    found.truncate(limit);
    Ok(to_value(Output {
        changes: found,
        total,
        truncated: total > limit,
        unreadable,
        warnings,
    }))
}

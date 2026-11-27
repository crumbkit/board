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

use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use board_repos::Date;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{body, get_text, readable, rows, text};
use crate::change::{
    attached, change_types, kind_of as kind_at, parsed, plan_at, referenced, referenced_in_walk,
    today,
};
use crate::diff::{conflicts, mounted};
use crate::flow::{self, Actor, Kind as CheckKind, of_change};
use crate::gates::{
    Check, Observed, REPORT_TYPE, RUN_TYPE, Read as ReadKind, Status, fails_unread, gate, objects,
    ran_on, reads, reviewed_head, same_commit,
};
use crate::owed::{OWED_DAYS, Owed, owed_of};
use crate::phases::{observe_in, plan_file};
use crate::plan::Answers;
use crate::records::{ChangeRecord, DONE, DROPPED, PlanRecord};
use crate::{PLUGIN, Warning, to_value};

/// The default `limit` (STANDARDS 23).
const READY_LIMIT: usize = 100;
/// How long the merge check of one change may take.
const MERGE_DEADLINE: Duration = Duration::from_secs(1);
/// How long the merge checks of one read may take in all, well inside the
/// 5 seconds the board-plans mod gives the whole read.
const MERGE_TOTAL: Duration = Duration::from_secs(1);

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
/// its head conflicts with the branch its mount follows, or reviews are
/// attached and none covers its head.
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
/// `done` says whether the change is done and owes only a recheck.
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
    done: bool,
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

/// What the merge checks of one read may still spend: each change's check
/// gets the time left, at most [`MERGE_DEADLINE`], and none runs once the
/// read's merge checks have spent [`MERGE_TOTAL`] from the first.
#[derive(Default)]
struct MergeBudget {
    until: std::cell::Cell<Option<std::time::Instant>>,
}

impl MergeBudget {
    /// The time the next merge check may take, or none when it is spent.
    fn next(&self) -> Option<Duration> {
        let now = std::time::Instant::now();
        let until = self.until.get().unwrap_or(now + MERGE_TOTAL);
        self.until.set(Some(until));
        let left = until.saturating_duration_since(now);
        (!left.is_zero()).then(|| left.min(MERGE_DEADLINE))
    }
}

/// Why the change at `path` is failing, or none, and a warning when git
/// could not say whether its head merges. A change whose merge check git
/// cannot answer is not marked failing.
fn failing_of(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    budget: &MergeBudget,
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
    let merged = match (mounted(host, &change.mount)?, budget.next()) {
        (Ok(at), Some(deadline)) => conflicts(host, &at, head, deadline),
        (Ok(_), None) => Err(format!(
            "the merge checks of this read spent their {} second",
            MERGE_TOTAL.as_secs()
        )),
        (Err(why), _) => Err(why),
    };
    match merged {
        Ok(true) => return Ok((Some(Failing::Conflict), None)),
        Ok(false) => {}
        Err(why) => {
            let warning = Warning {
                code: "changes::ready::merge_unread",
                detail: format!(
                    "whether {path} merges cleanly is not known, so it is not marked failing: {why}"
                ),
            };
            return Ok((None, Some(warning)));
        }
    }
    // Reports are kept by path, so none is the newest; the review is stale
    // only when no attached report covers the head.
    let mut reports = records.of_type(REPORT_TYPE).peekable();
    let stale = reports.peek().is_some()
        && !reports.any(|(_, report)| same_commit(reviewed_head(report).as_ref(), head));
    Ok((stale.then_some(Failing::Stale), None))
}

/// The row of a change in the last phase of its flow, when it waits only
/// on the owner, and a warning when its merge check could not be read.
fn ready_in_land(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    (flow, phase): (flow::Flow, &flow::Phase),
    asked: &[Asked],
    failing: Option<&MergeBudget>,
) -> Result<(Option<Ready>, Option<Warning>), PluginError> {
    let observed = observe_in(host, path, change, Some(flow))?;
    let checks = gate(&observed);
    let blocked = (checks.iter()).any(|each| each.blocking() && each.actor != Actor::Owner);
    let owners: Vec<&Check> = (checks.iter())
        .filter(|each| each.status == Status::Missing && each.actor == Actor::Owner)
        .collect();
    if blocked || (owners.is_empty() && asked.is_empty()) {
        return Ok((None, None));
    }
    let first = owners.iter().find(|each| each.blocks).or(owners.first());
    let (kind, check, waits_on) = match first {
        Some(each) => (
            kind_of(phase, each),
            Some(each.check.clone()),
            each.detail.clone(),
        ),
        None => (ReadyKind::LandChoice, None, asked[0].question.clone()),
    };
    let (failing, warning) = match failing {
        Some(budget) => failing_of(host, path, change, budget)?,
        None => (None, None),
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
        done: false,
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
        done: true,
    })
}

/// What the gate of a done change's owed checks in `flow` reads. When
/// those checks read only the change's references and plan, as an
/// understanding check does, only those are read, which leaves out the
/// board's own location that `observe_in` resolves and only a remedy names;
/// otherwise `observe_in` reads them.
fn observe_owed(
    host: &dyn Host,
    path: &str,
    change: &ChangeRecord,
    flow: flow::Flow,
) -> Result<Observed, PluginError> {
    let wanted = reads((flow.phase(&change.phase)).map_or(&[], |phase| phase.checks.as_slice()));
    if !(wanted.iter()).all(|read| matches!(read, ReadKind::References | ReadKind::Plan)) {
        return observe_in(host, path, change, Some(flow));
    }
    let cited = attached(host, path)?;
    let records = referenced(host, &cited)?;
    let mut found = Observed::new(path, change.clone(), kind_at(host, path)?, records);
    found.flow = Some(flow);
    found.cited = cited;
    if wanted.contains(&ReadKind::Plan) {
        found.plan_file = plan_file(host, change)?;
        found.plan = match &found.plan_file {
            Some(file) => get_text(host, file)?,
            None => None,
        };
        let named = (change.plan.as_deref().map(|plan| plan_at(host, plan))).transpose()?;
        if let Some((record, _)) = named.flatten() {
            found.answers = record.answers.unwrap_or_default();
        }
    }
    Ok(found)
}

/// Whether the check is the owner's.
fn owners(each: &flow::Check) -> bool {
    each.actor == Some(Actor::Owner)
}

/// A copy of `flow`, which the gate takes by value.
fn copied(flow: &flow::Flow) -> Option<flow::Flow> {
    serde_json::from_value(serde_json::to_value(flow).ok()?).ok()
}

/// The flows the walk reads, each read once in one query for every flow
/// path the changes name.
#[derive(Default)]
struct Read(BTreeMap<String, Option<flow::Flow>>);

impl Read {
    /// Reads at once the flows at `paths`; a path that holds no flow Board
    /// reads is kept as none.
    fn load(&mut self, host: &dyn Host, paths: &BTreeSet<String>) -> Result<(), PluginError> {
        if paths.is_empty() {
            return Ok(());
        }
        let listed: Vec<String> = paths.iter().map(|each| literal(each)).collect();
        let sql = format!(
            "SELECT e.logical, e.type, e.code, b.body FROM entries AS e LEFT JOIN bodies AS b \
             ON b.blob = e.blob WHERE e.logical IN ({})",
            listed.join(", ")
        );
        for row in rows(host, &sql)? {
            let Some(path) = text(row.first()) else {
                continue;
            };
            let flow = (text(row.get(1)) == Some(flow::Flow::TYPE) && readable(row.get(2)))
                .then(|| serde_json::from_value(Value::Object(body(row.get(3)))).ok())
                .flatten();
            self.0.insert(path.to_owned(), flow);
        }
        for path in paths {
            self.0.entry(path.clone()).or_insert(None);
        }
        Ok(())
    }

    /// The flow the change at `path` follows, or none when it follows none
    /// or Board cannot read it.
    fn of(&mut self, host: &dyn Host, path: &str, change: &ChangeRecord) -> Option<flow::Flow> {
        let read = || of_change(host, path, change).ok().flatten();
        match change.flow.as_ref().filter(|_| !change.v1) {
            Some(copy) => (self.0.entry(copy.clone()).or_insert_with(read))
                .as_ref()
                .and_then(copied),
            None => read(),
        }
    }
}

/// What one change's row reads beside the change and its flow: its plan's
/// answers, the open questions that block it, and the read's merge budget
/// when it reads why changes are failing.
struct Given<'a> {
    answers: Answers,
    questions: &'a [Asked],
    budget: Option<&'a MergeBudget>,
}

/// The row of the change at `path`, which follows `flow`, when it waits
/// only on the owner, and a warning when its merge check could not be
/// read. A change no check of the owner's can apply to, and no question
/// blocks, is passed over before its gate is read.
fn row_of(
    host: &dyn Host,
    path: &str,
    mut change: ChangeRecord,
    mut flow: flow::Flow,
    given: &Given<'_>,
) -> Result<(Option<Ready>, Option<Warning>), PluginError> {
    // The route and answers are judged as the gate judges them, so a `when`
    // the gate decides on the plan's text is left to the gate.
    let applies =
        |each: &flow::Check| owners(each) && !fails_unread(each, change.route, Some(given.answers));
    let done = change.phase == DONE;
    let Some(last) = flow.phases.last_mut() else {
        return Ok((None, None));
    };
    if done {
        // Only the owner's checks that do not block are read, so a done
        // change's diff and pull request are not.
        last.checks
            .retain(|each| each.blocks == Some(false) && applies(each));
        if last.checks.is_empty() {
            return Ok((None, None));
        }
        change.phase.clone_from(&last.name);
        let observed = observe_owed(host, path, &change, flow)?;
        let row = owed_of(&observed).and_then(|owed| recheck(host, owed, given.questions));
        return Ok((row, None));
    }
    if last.name != change.phase || (given.questions.is_empty() && !last.checks.iter().any(applies))
    {
        return Ok((None, None));
    }
    // The gate takes the flow; its copy names the kinds of the checks.
    let Some(copy) = copied(&flow) else {
        return Ok((None, None));
    };
    let Some(last) = copy.phases.last() else {
        return Ok((None, None));
    };
    ready_in_land(
        host,
        path,
        &change,
        (flow, last),
        given.questions,
        given.budget,
    )
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
    // Each change that is still going, and each done in the last OWED_DAYS
    // days, with its plan's answers, so a change no check of the owner's
    // can apply to is passed over before its gate is read.
    let since = today(host).plus(-OWED_DAYS).to_string();
    let sql = format!(
        "SELECT c.logical, c.code, c.body, (SELECT json_extract(p.body, '$.answers') FROM records p \
         WHERE p.logical = json_extract(c.body, '$.plan') AND p.type = {plan} LIMIT 1) \
         FROM records c WHERE c.type IN ({types}) AND (json_extract(c.body, '$.phase') \
         NOT IN ('{DONE}', '{DROPPED}') OR (json_extract(c.body, '$.phase') = '{DONE}' AND \
         json_extract(c.body, '$.touched') >= '{since}')) ORDER BY c.logical",
        plan = literal(PlanRecord::TYPE),
        types = change_types(),
    );
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let mut unreadable = 0;
    let mut flows = Read::default();
    let budget = failing.then(MergeBudget::default);
    let listed = rows(host, &sql)?;
    let named: BTreeSet<String> = (listed.iter())
        .filter_map(|row| parsed(row.get(2)))
        .filter(|change| !change.v1)
        .filter_map(|change| change.flow)
        .collect();
    flows.load(host, &named)?;
    for row in listed {
        let change = (readable(row.get(1))).then(|| parsed(row.get(2))).flatten();
        let (Some(path), Some(change)) = (text(row.first()), change) else {
            unreadable += 1;
            continue;
        };
        if change.phase == DROPPED || only.as_ref().is_some_and(|only| !only.contains(path)) {
            continue;
        }
        let answers: Answers = (text(row.get(3)))
            .and_then(|answers| serde_json::from_str(answers).ok())
            .unwrap_or_default();
        let Some(flow) = flows.of(host, path, &change) else {
            continue;
        };
        let given = Given {
            answers,
            questions: asked_of(path),
            budget: budget.as_ref(),
        };
        let (row, warning) = row_of(host, path, change, flow, &given)?;
        found.extend(row);
        warnings.extend(warning);
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

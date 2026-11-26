//! The `owed` capability: each change done in the last [`OWED_DAYS`] days
//! whose flow's last phase has a check that does not block and is still
//! missing, such as the rechecks of a data understanding check, which the
//! owner is asked after the merge.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{readable, rows, text};
use crate::change::{attached_each, change_types, parsed, plans_at, referenced, today};
use crate::flow::{self, Flow, at_paths, development, gone};
use crate::gates::remedy::{self, PLACEHOLDERS};
use crate::gates::{
    Check, Observed, Read, Status, UNDERSTANDING_TYPE, fails_unread, interpret, objects, reads,
};
use crate::phases::{Known, board_root, observe_known};
use crate::plan::Answers;
use crate::records::{ChangeRecord, DONE, PlanRecord};
use crate::{PLUGIN, to_value};

/// How many days after its last touch a done change is still listed.
pub(crate) const OWED_DAYS: i64 = 14;
/// The default `limit` (STANDARDS 23).
const OWED_LIMIT: usize = 100;
/// The kinds of understanding question that are asked again.
const ASKED_AGAIN: [&str; 2] = ["predict", "edge"];

/// The changes that still owe a check after landing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OwedInput")]
pub(crate) struct Input {
    #[serde(default = "owed_limit")]
    #[schemars(
        range(min = 1, max = 10_000),
        description = "The most changes to return, 1 to 10,000; 100 when left out."
    )]
    limit: usize,
}

fn owed_limit() -> usize {
    OWED_LIMIT
}

/// A predict or edge question of an attached understanding check, which
/// the owner is asked again, worded differently.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Asked {
    record: String,
    question: String,
}

/// A done change and the checks of its last phase that do not block and
/// are missing, each with its detail and remedy, and the questions to ask
/// the owner again.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Owed {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) touched: Option<String>,
    pub(crate) checks: Vec<Check>,
    questions: Vec<Asked>,
    /// The change's newest pull request, which `changes.ready` names.
    #[serde(skip)]
    pub(crate) pull: Option<String>,
    /// The change's head commit, which `changes.ready` names.
    #[serde(skip)]
    pub(crate) head: Option<String>,
}

/// The done changes that owe a check, newest touch first.
///
/// `total` counts every such change and `truncated` says whether the limit
/// left some out. `unreadable` is how many done changes were left out
/// because Board marks them degraded.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "OwedOutput")]
pub(crate) struct Output {
    changes: Vec<Owed>,
    total: usize,
    truncated: bool,
    unreadable: usize,
}

/// What the change `observed` still owes: each check of its flow's last
/// phase that does not block and is missing, or none when it owes nothing.
pub(crate) fn owed_of(observed: &Observed) -> Option<Owed> {
    let last = observed.flow.as_ref()?.phases.last()?;
    let checks: Vec<Check> = (interpret(observed, &last.checks).into_iter())
        .filter(|each| !each.blocks && each.status == Status::Missing)
        .collect();
    if checks.is_empty() {
        return None;
    }
    let questions = (observed.records.of_type(UNDERSTANDING_TYPE))
        .filter(|(_, body)| body.get("change").and_then(Value::as_str) == Some(&observed.path))
        .flat_map(|(path, body)| {
            (objects(body, "questions"))
                .filter(|asked| {
                    (asked.get("kind").and_then(Value::as_str))
                        .is_some_and(|kind| ASKED_AGAIN.contains(&kind))
                })
                .filter_map(move |asked| {
                    Some(Asked {
                        record: path.to_owned(),
                        question: asked.get("question")?.as_str()?.to_owned(),
                    })
                })
        })
        .collect();
    Some(Owed {
        path: observed.path.clone(),
        title: observed.change.title.clone(),
        touched: observed.change.touched.clone(),
        checks,
        questions,
        pull: observed.change.pulls().last().cloned(),
        head: observed.change.head.clone(),
    })
}

/// Each change done in the last [`OWED_DAYS`] days that owes a check,
/// newest touch first, and how many done changes Board does not read.
///
/// A change is judged first from what the walk holds: the flow it follows,
/// read once for each flow path, then its route and its plan's answers,
/// read in one query for every change still in question. Only a change
/// left with a check whose `when` holds, or that the answers cannot
/// settle, is observed, and those changes' attached records are read
/// together. The board's root is read only when such a check names
/// `{board}`.
pub(crate) fn owing(host: &dyn Host) -> Result<(Vec<Owed>, usize), PluginError> {
    let (done, unreadable) = done_since(host)?;
    let (flows, asked) = with_checks(host, done)?;
    let left = answered(host, &flows, asked)?;
    let names_board = (left.iter())
        .flat_map(|each| each.flow.phases.last().into_iter())
        .flat_map(|last| &last.checks)
        .any(names_board);
    let board = if names_board { board_root(host)? } else { None };
    let citing: Vec<&str> = (left.iter())
        .filter(|each| reads(each.checks()).contains(&Read::References))
        .map(|each| each.path.as_str())
        .collect();
    let mut cited = attached_each(host, &citing)?;
    let all: Vec<String> = cited.values().flatten().cloned().collect();
    let records = referenced(host, &all)?;
    let mut found = Vec::new();
    for Left {
        path,
        change,
        flow,
        plan,
    } in left
    {
        let cited = cited.remove(&path).unwrap_or_default();
        let records = records.only(&cited);
        let known = Known {
            plan,
            board: board.clone(),
            cited,
            records,
        };
        let observed = observe_known(host, &path, &change, Some(flow), Some(known))?;
        found.extend(owed_of(&observed));
    }
    Ok((found, unreadable))
}

/// Each change done in the last [`OWED_DAYS`] days that follows a flow,
/// newest touch first, and how many done changes Board does not read.
fn done_since(host: &dyn Host) -> Result<(Vec<(String, ChangeRecord)>, usize), PluginError> {
    let since = today(host).plus(-OWED_DAYS).to_string();
    let sql = format!(
        "SELECT logical, code, body FROM records WHERE type IN ({}) AND \
         json_extract(body, '$.phase') = '{DONE}' AND json_extract(body, '$.touched') >= '{since}' \
         ORDER BY json_extract(body, '$.touched') DESC, logical",
        change_types()
    );
    let mut unreadable = 0;
    let mut done = Vec::new();
    for row in rows(host, &sql)? {
        let change = (readable(row.get(1))).then(|| parsed(row.get(2))).flatten();
        let (Some(path), Some(change)) = (text(row.first()), change) else {
            unreadable += 1;
            continue;
        };
        if change.v1 || change.flow.is_some() {
            done.push((path.to_owned(), change));
        }
    }
    Ok((done, unreadable))
}

/// The last phase of each flow with only its checks that do not block, by
/// the flow's path, or by none for the flow a v1 change follows; none when
/// it has no such check.
type Flows = HashMap<Option<String>, Option<Flow>>;

/// The flow path a change's flow is kept under in [`Flows`].
fn key(change: &ChangeRecord) -> Option<String> {
    change.flow.clone().filter(|_| !change.v1)
}

/// The flows of the changes `done`, read in one query, and those changes
/// with a check that does not block and whose `when` does not fail on
/// their route. A flow path that holds no flow Board reads is
/// `changes::flow::unreadable`, as `of_change` says.
fn with_checks(
    host: &dyn Host,
    done: Vec<(String, ChangeRecord)>,
) -> Result<(Flows, Vec<(String, ChangeRecord)>), PluginError> {
    let named: HashSet<&str> = (done.iter())
        .filter(|(_, change)| !change.v1)
        .filter_map(|(_, change)| change.flow.as_deref())
        .collect();
    let mut read = at_paths(host, named)?;
    let mut flows = Flows::new();
    let mut asked = Vec::new();
    for (path, change) in done {
        let flow = match flows.entry(key(&change)) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(empty) => {
                let flow = match empty.key() {
                    None => development(),
                    Some(copy) => read.remove(copy).ok_or_else(|| gone(&path, copy))??,
                };
                empty.insert(after(flow))
            }
        };
        if (flow.as_ref()).is_some_and(|flow| may_apply(flow, &change, None)) {
            asked.push((path, change));
        }
    }
    Ok((flows, asked))
}

/// A done change left to observe, in its flow's last phase, with that
/// phase's checks that may apply and its plan record.
struct Left {
    path: String,
    change: ChangeRecord,
    flow: Flow,
    plan: Option<PlanRecord>,
}

impl Left {
    fn checks(&self) -> &[flow::Check] {
        self.flow.phases.last().map_or(&[], |last| &last.checks)
    }
}

/// Each of the changes `asked` with a check whose `when` does not fail on
/// its plan's answers, read in one query.
fn answered(
    host: &dyn Host,
    flows: &Flows,
    asked: Vec<(String, ChangeRecord)>,
) -> Result<Vec<Left>, PluginError> {
    let named: Vec<&str> = (asked.iter())
        .filter_map(|(_, change)| change.plan.as_deref())
        .collect();
    let plans = plans_at(host, &named)?;
    let mut left = Vec::new();
    for (path, mut change) in asked {
        let plan = (change.plan.as_ref())
            .and_then(|plan| plans.get(plan))
            .map(|(record, _)| record);
        let answers = plan.and_then(|plan| plan.answers).unwrap_or_default();
        let Some(Some(flow)) = flows.get(&key(&change)) else {
            continue;
        };
        if !may_apply(flow, &change, Some(answers)) {
            continue;
        }
        let mut flow = flow.clone();
        if let Some(last) = flow.phases.last_mut() {
            last.checks
                .retain(|each| !fails_unread(each, change.route, Some(answers)));
            change.phase.clone_from(&last.name);
        }
        left.push(Left {
            path,
            change,
            flow,
            plan: plan.cloned(),
        });
    }
    Ok(left)
}

/// `flow` with only its last phase's checks that do not block, or none
/// when it has no such check.
fn after(mut flow: Flow) -> Option<Flow> {
    let last = flow.phases.last_mut()?;
    last.checks.retain(|each| each.blocks == Some(false));
    (!last.checks.is_empty()).then_some(flow)
}

/// Whether a check of the last phase of `flow` has a `when` that does not
/// fail on the change's route or, when given, on its plan's `answers`.
fn may_apply(flow: &Flow, change: &ChangeRecord, answers: Option<Answers>) -> bool {
    (flow.phases.last().into_iter())
        .flat_map(|last| &last.checks)
        .any(|each| !fails_unread(each, change.route, answers))
}

/// What stands in for the board's root while [`names_board`] fills a
/// remedy.
const ROOT: &str = "\u{0}board";

/// Whether the check names `{board}`, in a template of its own or in the
/// remedy its kind gives, so its owed entry needs the board's root.
fn names_board(each: &flow::Check) -> bool {
    let own = serde_json::to_string(each).is_ok_and(|text| text.contains("{board}"));
    let values = PLACEHOLDERS.map(|name| (name, (name == "board").then(|| ROOT.to_owned())));
    own || remedy::of(each, &values).contains(ROOT)
}

pub(crate) fn owed(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { limit } = PluginError::read_input(PLUGIN, "owed", input)?;
    let (mut changes, unreadable) = owing(host)?;
    let total = changes.len();
    changes.truncate(limit);
    Ok(to_value(Output {
        changes,
        total,
        truncated: total > limit,
        unreadable,
    }))
}

#[cfg(test)]
mod tests;

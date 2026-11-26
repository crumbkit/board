//! The `owed` capability: each change done in the last [`OWED_DAYS`] days
//! whose flow's last phase has a check that does not block and is still
//! missing, such as the rechecks of a data understanding check, which the
//! owner is asked after the merge.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use board_model::parse_json;
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{readable, rows, text, tree_codes};
use crate::change::{NOT_FOUND, Referenced, change_types, parsed, referenced, today};
use crate::flow::{self, Flow, at_paths, development, gone};
use crate::gates::remedy::{self, PLACEHOLDERS};
use crate::gates::{Check, Observed, Read, Status, UNDERSTANDING_TYPE, interpret, objects, reads};
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
    title: String,
    touched: Option<String>,
    checks: Vec<Check>,
    questions: Vec<Asked>,
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
    let mut cited = attached_each(
        host,
        (left.iter())
            .filter(|each| reads(each.checks()).contains(&Read::References))
            .map(|each| each.path.as_str()),
    )?;
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
        let records = only(&records, &cited);
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
    let plans = plans_at(
        host,
        (asked.iter()).filter_map(|(_, change)| change.plan.as_deref()),
    )?;
    let mut left = Vec::new();
    for (path, mut change) in asked {
        let held = change.plan.as_ref().and_then(|plan| plans.get(plan));
        let answers = held.and_then(|plan| plan.answers).unwrap_or_default();
        let Some(Some(flow)) = flows.get(&key(&change)) else {
            continue;
        };
        if !may_apply(flow, &change, Some(answers)) {
            continue;
        }
        let mut flow = copied(flow);
        if let Some(last) = flow.phases.last_mut() {
            last.checks
                .retain(|each| !fails(each, &change, Some(answers)));
            change.phase.clone_from(&last.name);
        }
        let plan = held.and_then(|plan| serde_json::from_value(plan.body.clone()).ok());
        left.push(Left {
            path,
            change,
            flow,
            plan,
        });
    }
    Ok(left)
}

/// The records attached to each change at `paths`, by path, as
/// `change::attached` reads them for one, in one query.
fn attached_each<'a>(
    host: &dyn Host,
    paths: impl Iterator<Item = &'a str>,
) -> Result<HashMap<String, Vec<String>>, PluginError> {
    let prefixes: Vec<(&str, String)> =
        paths.map(|path| (path, format!("{path}/plans/"))).collect();
    let mut found: HashMap<String, Vec<String>> = HashMap::new();
    if prefixes.is_empty() {
        return Ok(found);
    }
    let under: Vec<String> = (prefixes.iter())
        .map(|(_, prefix)| {
            format!(
                "substr(logical, 1, {}) = {}",
                prefix.chars().count(),
                literal(prefix)
            )
        })
        .collect();
    let sql = format!(
        "SELECT logical, body FROM records WHERE type = {} AND ({}) AND ({}) ORDER BY logical",
        literal(PlanRecord::TYPE),
        sound(),
        under.join(" OR ")
    );
    for row in rows(host, &sql)? {
        let Some(logical) = text(row.first()) else {
            continue;
        };
        let records = plan_in(row.get(1)).and_then(|plan| plan.records);
        for (path, prefix) in &prefixes {
            if !logical.starts_with(prefix.as_str()) {
                continue;
            }
            let listed = found.entry((*path).to_owned()).or_default();
            for each in records.iter().flatten() {
                if !listed.contains(each) {
                    listed.push(each.clone());
                }
            }
        }
    }
    Ok(found)
}

/// The part of `all` that `refs` name, in their order, each once, as
/// `change::referenced` reads `refs` alone.
fn only(all: &Referenced, refs: &[String]) -> Referenced {
    let mut found = Referenced::default();
    for each in refs {
        if found.records.iter().any(|(seen, _)| seen == each)
            || found.unreadable.iter().any(|(seen, _)| seen == each)
        {
            continue;
        }
        if let Some(record) = all.get(each) {
            found.records.push((each.clone(), record.clone()));
            continue;
        }
        let code = (all.unreadable.iter())
            .find_map(|(seen, code)| (seen == each).then(|| code.clone()))
            .unwrap_or_else(|| NOT_FOUND.to_owned());
        found.unreadable.push((each.clone(), code));
        if let Some(kind) = all.unreadable_types.get(each) {
            found.unreadable_types.insert(each.clone(), kind.clone());
        }
    }
    found
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
        .any(|each| !fails(each, change, answers))
}

/// Whether the check's `when` fails on the change's route or on `answers`.
///
/// The gate reads a `when`'s route first, then its section patterns, then
/// its answers. A pattern it cannot read makes the check missing, so a
/// `when` with section patterns is left to the gate whatever the answers.
fn fails(each: &flow::Check, change: &ChangeRecord, answers: Option<Answers>) -> bool {
    let Some(when) = &each.when else {
        return false;
    };
    if let Some(routes) = &when.route
        && !(change.route).is_some_and(|route| routes.contains(&route))
    {
        return true;
    }
    if when.section.is_some() || when.section_not.is_some() {
        return false;
    }
    let Some(answers) = answers else {
        return false;
    };
    let unmet = when.answers.is_some_and(|wanted| !answers.meet(wanted));
    let unless = when.answers_not.is_some_and(|unless| {
        (unless.ux.is_none() || answers.ux == unless.ux)
            && (unless.data.is_none() || answers.data == unless.data)
            && (unless.lifecycle.is_none() || answers.lifecycle == unless.lifecycle)
    });
    unmet || unless
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

/// A plan record's typed answers and its body, which the change's observed
/// plan reads.
struct Held {
    answers: Option<Answers>,
    body: Value,
}

/// The plan record at each of `paths` that Board reads, by path, in one
/// query, as `plan_at` reads one.
fn plans_at<'a>(
    host: &dyn Host,
    paths: impl Iterator<Item = &'a str>,
) -> Result<HashMap<String, Held>, PluginError> {
    let listed: Vec<String> = paths.map(literal).collect();
    let mut found = HashMap::new();
    if listed.is_empty() {
        return Ok(found);
    }
    let sql = format!(
        "SELECT logical, body FROM records WHERE type = {} AND ({}) AND logical IN ({})",
        literal(PlanRecord::TYPE),
        sound(),
        listed.join(", ")
    );
    for row in rows(host, &sql)? {
        let Some(path) = text(row.first()) else {
            continue;
        };
        let Some(body) = text(row.get(1)).and_then(|body| parse_json(body.as_bytes()).ok()) else {
            continue;
        };
        let Some(plan) = serde_json::from_value::<PlanRecord>(body.clone()).ok() else {
            continue;
        };
        (found.entry(path.to_owned())).or_insert(Held {
            answers: plan.answers,
            body,
        });
    }
    Ok(found)
}

/// The plan record a `body` column holds, or none when it does not read as
/// one.
fn plan_in(body: Option<&Value>) -> Option<PlanRecord> {
    let body = parse_json(text(body)?.as_bytes()).ok()?;
    serde_json::from_value(body).ok()
}

/// The SQL condition that a row's code leaves its record read: none, or a
/// code only the tree around it gives.
fn sound() -> String {
    format!("code IS NULL OR code IN ({})", tree_codes())
}

/// A copy of `flow`, which a flow Board read always gives.
fn copied(flow: &Flow) -> Flow {
    serde_json::to_value(flow)
        .and_then(serde_json::from_value)
        .expect("a flow Board read writes and reads back as itself")
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

//! The `owed` capability: each change done in the last [`OWED_DAYS`] days
//! whose flow's last phase has a check that does not block and is still
//! missing, such as the rechecks of a data understanding check, which the
//! owner is asked after the merge.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::board::{readable, rows, text};
use crate::change::{change_types, parsed, today};
use crate::flow::of_change;
use crate::gates::{Check, Observed, Status, UNDERSTANDING_TYPE, interpret, objects};
use crate::phases::observe_in;
use crate::records::DONE;
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
pub(crate) fn owing(host: &dyn Host) -> Result<(Vec<Owed>, usize), PluginError> {
    let since = today(host).plus(-OWED_DAYS).to_string();
    let sql = format!(
        "SELECT logical, code, body FROM records WHERE type IN ({}) AND \
         json_extract(body, '$.phase') = '{DONE}' AND json_extract(body, '$.touched') >= '{since}' \
         ORDER BY json_extract(body, '$.touched') DESC, logical",
        change_types()
    );
    let mut found = Vec::new();
    let mut unreadable = 0;
    for row in rows(host, &sql)? {
        let change = (readable(row.get(1))).then(|| parsed(row.get(2))).flatten();
        let (Some(path), Some(mut change)) = (text(row.first()), change) else {
            unreadable += 1;
            continue;
        };
        let Some(mut flow) = of_change(host, path, &change)? else {
            continue;
        };
        let Some(last) = flow.phases.last_mut() else {
            continue;
        };
        // Only the checks that do not block are read, so a done change's
        // diff and pull request are not.
        last.checks.retain(|each| each.blocks == Some(false));
        if last.checks.is_empty() {
            continue;
        }
        change.phase.clone_from(&last.name);
        let observed = observe_in(host, path, &change, Some(flow))?;
        found.extend(owed_of(&observed));
    }
    Ok((found, unreadable))
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

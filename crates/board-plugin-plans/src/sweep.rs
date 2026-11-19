//! `plans.sweep`: withdraws each open question that no longer needs the
//! owner, with the reason, so that `plans.open` stops listing it. The record
//! stays, withdrawn.
//!
//! A question is dead when the change or task it blocks is done or dropped,
//! or when the session that asked it is archived. Board knows nothing of
//! sessions, so the caller names the archived ones. Of the questions left, a
//! question is answered when a question in its project that an answer names
//! asks the same words. Words match after lower-casing and keeping only
//! letters and digits (decision /projects/board/decisions/20261119T0039Z-a1de).
//! The sweep leaves open a question that repeats another open question
//! (decision /projects/board/decisions/20261119T1000Z-c4d9).

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Bulk, Host, Limit, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::open::{Waiting, project_of, waiting};
use crate::question::{self, OWN_PATH};

/// The reason the sweep's commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "sweep questions";
/// The most withdrawn questions the output lists (STANDARDS 23).
const LISTED: usize = 100;

/// Which questions to sweep, and whether to write.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "SweepInput")]
pub(crate) struct Input {
    /// Only questions below this logical path, such as /projects/board;
    /// every question when left out.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The `local_` ids of the sessions that are archived. A question one of
    /// them asked is withdrawn.
    #[schemars(inner(pattern("^local_[0-9a-f-]+$")))]
    archived: Vec<String>,
    /// Withdraw the questions found, in one commit; without it nothing is
    /// written.
    apply: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            path: "/".to_owned(),
            archived: Vec::new(),
            apply: false,
        }
    }
}

/// What the sweep withdrew, or would withdraw without `apply`.
///
/// `withdrawn` lists at most 100 questions, with `total` and `truncated`.
/// `counts` gives how many of each kind, and `remaining` how many open
/// questions are left.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SweepOutput")]
pub(crate) struct Output {
    applied: bool,
    withdrawn: Vec<Withdrawn>,
    total: usize,
    truncated: bool,
    counts: BTreeMap<&'static str, usize>,
    remaining: usize,
}

/// A question the sweep withdraws, why, and the reason it records.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Withdrawn {
    path: String,
    /// `done`, `dropped`, `archived` or `answered`.
    kind: &'static str,
    reason: String,
}

/// Each kind of withdrawal, so that `counts` names every kind, even at 0.
const KINDS: [&str; 4] = ["done", "dropped", "archived", "answered"];

pub(crate) fn sweep(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        archived,
        apply,
    } = PluginError::read_input("plans", "sweep", input)?;
    let archived: BTreeSet<String> = archived.into_iter().collect();
    let (open, _) = waiting(host, &path)?;
    let answered = answered(host)?;
    let found = find(&open, &archived, &answered);
    if apply && !found.is_empty() {
        write(host, &found)?;
    }
    let mut counts: BTreeMap<&'static str, usize> = KINDS.iter().map(|kind| (*kind, 0)).collect();
    for each in &found {
        *counts.entry(each.kind).or_default() += 1;
    }
    let total = found.len();
    let output = Output {
        applied: apply,
        remaining: open.len() - total,
        withdrawn: found.into_iter().take(LISTED).collect(),
        total,
        truncated: total > LISTED,
        counts,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// Withdraws each of `found`, after reading it again to check it is still
/// open. A put batch may not cross a mount, so the questions of each board,
/// named by the mount prefix of their paths, go in a batch of their own. A
/// board's batch is one commit or none; when a later board's batch is
/// refused, the boards before it keep their withdrawals, the refusal lists
/// them in `written`, and its hint says to sweep again.
fn write(host: &dyn Host, found: &[Withdrawn]) -> Result<(), PluginError> {
    let mut boards: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for each in found {
        let mut asked = question::read(host, "sweep", &each.path)?;
        (asked.still_open("sweep", &each.path)).map_err(|mut error| {
            error.hint =
                Some("run `board plans sweep` again; it reads each question afresh".to_owned());
            error
        })?;
        let prefix = (each.path.strip_suffix(asked.own.as_str())).unwrap_or_default();
        asked.fields.insert("status".to_owned(), json!("withdrawn"));
        (asked.fields).insert("withdrawn_reason".to_owned(), json!(each.reason));
        let line = json!({"path": each.path, "expect": asked.blob, "record": asked.fields});
        boards.entry(prefix.to_owned()).or_default().push(line);
    }
    let mut written: Vec<String> = Vec::new();
    let refused = |refusal: &board_model::Refusal, written: &[String]| {
        let mut error = PluginError::board_refused("plans", "put --batch --bulk", refusal);
        (error.fields).insert("written".to_owned(), json!(written));
        error.hint = (error.hint).map(|hint| {
            format!("{hint}, then run `board plans sweep` again to withdraw the questions left")
        });
        error
    };
    let bulk = Bulk::new(BULK).map_err(|error| refused(&error.refusal(), &written))?;
    for (board, lines) in boards {
        (host.put_bulk(&lines, &bulk)).map_err(|error| refused(&error.refusal(), &written))?;
        written.push(if board.is_empty() {
            "/".to_owned()
        } else {
            board
        });
    }
    Ok(())
}

/// The questions to withdraw among `open`, sorted by path.
fn find(open: &[Waiting], archived: &BTreeSet<String>, answered: &Answered) -> Vec<Withdrawn> {
    let mut found = Vec::new();
    for question in open {
        let reason = match (&question.ended, &question.blocks) {
            (Some(ended), Some(blocks)) => Some((
                if ended == "done" { "done" } else { "dropped" },
                format!("the work it blocks, {blocks}, is {ended}."),
            )),
            _ if archived.contains(&question.session) => Some((
                "archived",
                format!(
                    "the session that asked it, {}, is archived.",
                    question.session
                ),
            )),
            _ => {
                let key = (question.project.clone(), words(&question.question));
                answered.get(&key).map(|(earlier, answer)| {
                    let why = format!("{earlier} asked the same and was answered at {answer}.");
                    ("answered", why)
                })
            }
        };
        if let Some((kind, why)) = reason {
            found.push(withdrawn(question, kind, &why));
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

fn withdrawn(question: &Waiting, kind: &'static str, why: &str) -> Withdrawn {
    Withdrawn {
        path: question.path.clone(),
        kind,
        reason: format!("plans.sweep: {why}"),
    }
}

/// `text` lower-cased, with only its runs of letters and digits, one space
/// apart.
pub(crate) fn words(text: &str) -> String {
    let lower = text.to_lowercase();
    let runs = lower.split(|each: char| !each.is_alphanumeric());
    runs.filter(|run| !run.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Answered questions keyed by their project and words, each with the
/// question's path and the answer's.
type Answered = BTreeMap<(String, String), (String, String)>;

/// Each question an answer names, by its project and words, with the
/// question's path and the answer's. Of the questions a project asks with
/// the same words, the one first by logical path is kept.
fn answered(host: &dyn Host) -> Result<Answered, PluginError> {
    let report = (host.query(&answered_sql(), Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    let mut found = BTreeMap::new();
    for row in &report.rows {
        let [
            Value::String(path),
            Value::String(text),
            Value::String(answer),
        ] = &row[..]
        else {
            continue;
        };
        let key = (project_of(path), words(text));
        found
            .entry(key)
            .or_insert_with(|| (path.clone(), answer.clone()));
    }
    Ok(found)
}

/// Every question an answer names, with its words and that answer's path,
/// by question path. An answer names a question as `question::NAMES` says,
/// each way looked up by key as `plans.open` does.
fn answered_sql() -> String {
    format!(
        "WITH named AS MATERIALIZED (SELECT a.source AS source, \
         CAST(json_extract(a.body, '$.question') AS TEXT) AS target, a.logical AS answer \
         FROM records a WHERE a.type = 'plans/answer/v1') \
         SELECT logical, words, answer FROM (SELECT q.logical AS logical, \
         json_extract(q.body, '$.question') AS words, COALESCE( \
         (SELECT n.answer FROM named n WHERE n.source = 'worktree' AND n.target = q.logical), \
         (SELECT n.answer FROM named n WHERE n.source = q.source AND n.target = q.logical), \
         (SELECT n.answer FROM named n WHERE n.source = q.source AND n.target = {OWN_PATH})) AS answer \
         FROM records q WHERE q.type = 'plans/question/v1') \
         WHERE answer IS NOT NULL ORDER BY logical"
    )
}

#[cfg(test)]
mod tests;

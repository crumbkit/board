//! `plans.ask`: files a question for the owner, unless one already asks it.
//!
//! An open question in the same project that asks the same words takes the
//! asker into its `also_by` instead, so the owner sees one question and the
//! answer reaches every asker. A question in the same project that an
//! answer names and that asked the same words gives the asker that answer,
//! and the decisions that name the question or the answer, and nothing is
//! filed. Words match as `plans.sweep` matches them (decision
//! /projects/board/decisions/20261119T0039Z-a1de).

use board_model::{parse_json, utc_seconds};
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::open::waiting;
use crate::question;
use crate::records::QuestionRecord;
use crate::sweep::{answered, words};

/// The question to file and the project it concerns.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AskInput")]
pub(crate) struct Input {
    /// The project the question concerns, such as `board`; the question is
    /// filed under `/projects/<project>/questions/`.
    #[schemars(regex(pattern = r"^[a-z0-9][a-z0-9._-]*$"))]
    project: String,
    /// The question as a `plans/question/v1` holds it, with `question` and
    /// `by` naming the asking session. `$type`, `status` and `asked` may be
    /// left out; they are filled in as open and now.
    question: Map<String, Value>,
}

/// What became of the question.
///
/// `outcome` is `filed`, `merged` or `answered`, and `path` the question
/// filed, the open question it was merged into, or the earlier question that
/// was answered. `answer` and `decisions` are given only when it was
/// answered. `line` says what happened in one sentence.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AskOutput")]
pub(crate) struct Output {
    outcome: &'static str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<Answer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decisions: Option<Vec<String>>,
    line: String,
}

/// The owner's answer to the earlier question.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Answer {
    path: String,
    choice: Option<String>,
    words: String,
}

pub(crate) fn ask(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        project,
        mut question,
    } = PluginError::read_input("plans", "ask", input)?;
    let folder = format!("/projects/{project}");
    let text = (question.get("question").and_then(Value::as_str)).unwrap_or_default();
    let key = (folder.clone(), words(text));
    let asker = question.get("by").cloned().unwrap_or(Value::Null);
    let (open, _) = waiting(host, &folder)?;
    // `waiting` lists a project's questions critical first, then oldest
    // first, so the first match is the one the owner reads first.
    let same = (open.iter()).find(|each| each.project == folder && words(&each.question) == key.1);
    let output = if let Some(same) = same {
        merge(host, same, &asker)?
    } else if let Some((earlier, answer)) = answered(host)?.remove(&key) {
        recall(host, earlier, answer)?
    } else {
        question.insert("$type".to_owned(), json!(QuestionRecord::TYPE));
        question.insert("status".to_owned(), json!("open"));
        let now = utc_seconds(host.settings().now);
        question.entry("asked").or_insert_with(|| json!(now));
        let line = json!({"new": format!("{folder}/questions/"), "record": question});
        let report = (host.put(&[line])).map_err(|error| {
            PluginError::board_refused("plans", "put --batch", &error.refusal())
        })?;
        let path =
            (report.writes.first()).map_or_else(String::new, |write| write.display.to_string());
        Output {
            outcome: "filed",
            line: format!("Filed {path}; the owner sees it in /ask."),
            path,
            answer: None,
            decisions: None,
        }
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings and lists"))
}

/// Adds `asker` to `same`'s `also_by`, unless it already asked.
fn merge(
    host: &dyn Host,
    same: &crate::open::Waiting,
    asker: &Value,
) -> Result<Output, PluginError> {
    let session = asker.get("session").and_then(Value::as_str);
    let known = session.is_some_and(|session| {
        same.session == session || same.also_asked.iter().any(|other| other == session)
    });
    if !known {
        let mut stored = question::read(host, "ask", &same.path)?;
        stored.still_open("ask", &same.path)?;
        let also = stored.fields.entry("also_by").or_insert_with(|| json!([]));
        if let Value::Array(also) = also {
            also.push(asker.clone());
        }
        let line = json!({"path": same.path, "expect": stored.blob, "record": stored.fields});
        (host.put(&[line])).map_err(|error| {
            PluginError::board_refused("plans", "put --batch", &error.refusal())
        })?;
    }
    Ok(Output {
        outcome: "merged",
        path: same.path.clone(),
        answer: None,
        decisions: None,
        line: format!(
            "{} already asks this, and its answer reaches you too.",
            same.path
        ),
    })
}

/// The answer at `answer` to `earlier`, and the decisions that name either.
fn recall(host: &dyn Host, earlier: String, answer: String) -> Result<Output, PluginError> {
    let refused = |error: board_ops::QueryError| {
        PluginError::board_refused("plans", "query", &error.refusal())
    };
    let (question, answered) = (literal(&earlier), literal(&answer));
    let sql = format!(
        "SELECT 'answer', body FROM records WHERE logical = {answered} \
         UNION ALL SELECT DISTINCT 'decision', d.logical FROM records d, json_each(d.body, '$.refs') r \
         WHERE d.type = 'plans/decision/v1' AND r.value IN ({question}, {answered})"
    );
    let report = host.query(&sql, Limit::ALL).map_err(refused)?;
    let mut body = Map::new();
    let mut decisions = Vec::new();
    for row in &report.rows {
        match (&row[0], &row[1]) {
            (Value::String(kind), Value::String(text)) if kind == "answer" => {
                if let Ok(Value::Object(read)) = parse_json(text.as_bytes()) {
                    body = read;
                }
            }
            (_, Value::String(path)) => decisions.push(path.clone()),
            _ => {}
        }
    }
    decisions.sort();
    let text = |name: &str| body.get(name).and_then(Value::as_str).map(str::to_owned);
    let words = text("words").unwrap_or_default();
    Ok(Output {
        outcome: "answered",
        line: format!("The owner already answered this at {answer}: {words}"),
        path: earlier,
        answer: Some(Answer {
            path: answer,
            choice: text("choice"),
            words,
        }),
        decisions: Some(decisions),
    })
}

#[cfg(test)]
mod tests;

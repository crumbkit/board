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

use crate::open::{Waiting, waiting};
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
    /// `by` naming the asking session, or the person who asks without one.
    /// `$type`, `status` and `asked` may be left out; they are filled in as
    /// open and now.
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
    question.insert("$type".to_owned(), json!(QuestionRecord::TYPE));
    question.insert("status".to_owned(), json!("open"));
    let now = utc_seconds(host.settings().now);
    question.entry("asked").or_insert_with(|| json!(now));
    let line = json!({"new": format!("{folder}/questions/"), "record": question.clone()});
    // Filing and merging admit the same questions: one its schema admits,
    // whose `by` therefore names a session or a person.
    let put_refused = |error: board_ops::PutError| {
        PluginError::board_refused("plans", "put --batch", &error.refusal())
    };
    host.check(std::slice::from_ref(&line))
        .map_err(put_refused)?;
    let text = (question.get("question").and_then(Value::as_str)).unwrap_or_default();
    let key = (folder.clone(), words(text));
    let asker = question.get("by").cloned().unwrap_or(Value::Null);
    let named = |field: &str| asker.get(field).and_then(Value::as_str).map(str::to_owned);
    let (session, person) = (named("session"), named("person"));
    let (open, _) = waiting(host, &folder)?;
    let same: Vec<&Waiting> = (open.iter())
        .filter(|each| each.project == folder && words(&each.question) == key.1)
        .collect();
    // A match the asker already asked is the one it waits on: by session
    // when the asker names one, and by person otherwise. Otherwise the
    // first match is the one the owner reads first, because `waiting` lists
    // a project's questions critical first, then oldest first.
    let known = (same.iter()).find(|each| match (&session, &person) {
        (Some(session), _) => {
            each.session.as_ref() == Some(session) || each.also_asked.contains(session)
        }
        (None, Some(person)) => {
            each.person.as_ref() == Some(person) || each.also_people.contains(person)
        }
        (None, None) => false,
    });
    let output = if let Some(known) = known {
        merged(&known.path)
    } else if let Some(first) = same.first() {
        merge(host, &first.path, &asker)?
    } else if let Some((earlier, answer)) = answered(host)?.remove(&key) {
        recall(host, earlier, answer)?
    } else {
        let report = host.put(&[line]).map_err(put_refused)?;
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

/// Adds `asker` to the `also_by` of the open question at `path`. A question
/// answered or withdrawn since `waiting` read it is refused with
/// `plans::ask::already_answered` or `plans::ask::not_open`, and nothing is
/// written.
fn merge(host: &dyn Host, path: &str, asker: &Value) -> Result<Output, PluginError> {
    let mut stored = question::read(host, "ask", path)?;
    stored.still_open("ask", path).map_err(|mut error| {
        error.hint = Some("run `board plans ask` again; it reads each question afresh".to_owned());
        error
    })?;
    let also = stored.fields.entry("also_by").or_insert_with(|| json!([]));
    if let Value::Array(also) = also {
        also.push(asker.clone());
    }
    let line = json!({"path": path, "expect": stored.blob, "record": stored.fields});
    (host.put(&[line]))
        .map_err(|error| PluginError::board_refused("plans", "put --batch", &error.refusal()))?;
    Ok(merged(path))
}

/// The output for an asker who now waits on the open question at `path`.
fn merged(path: &str) -> Output {
    Output {
        outcome: "merged",
        path: path.to_owned(),
        answer: None,
        decisions: None,
        line: format!("{path} already asks this, and its answer reaches you too."),
    }
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

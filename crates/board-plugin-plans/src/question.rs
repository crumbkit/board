//! What `answer` and `withdraw` share: reading one question and the answers
//! that name it, and refusing with a code of the calling capability.

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::Record;
use serde_json::{Map, Value, json};

use crate::records::{AnswerRecord, QuestionRecord};

/// SQL that is true when the answer `a` names the question `q`. A reference
/// in a mounted board is read from that board's root, so an answer names a
/// question by its path in the board both are in, and an answer in the root
/// board also names a mounted question by its logical path.
pub(crate) const NAMES: &str = "((a.source = 'worktree' AND json_extract(a.body, '$.question') = q.logical) \
     OR (a.source = q.source AND json_extract(a.body, '$.question') = '/' || \
     CASE WHEN q.path LIKE '%.json' THEN substr(q.path, 1, length(q.path) - 5) ELSE q.path END))";

/// The question at `path` and every answer that names it, at any id.
pub(crate) fn question_sql(path: &str) -> String {
    let path = literal(path);
    format!(
        "SELECT 'question' AS kind, logical, type, blob, body FROM records \
         WHERE logical = {path} \
         UNION ALL SELECT 'answer', a.logical, a.type, a.blob, NULL FROM records a, records q \
         WHERE q.logical = {path} AND a.type = '{}' AND {NAMES}",
        AnswerRecord::TYPE
    )
}

/// A question as `call` read it: its blob, its fields and the paths of the
/// answers that name it.
pub(crate) struct Asked {
    pub(crate) blob: String,
    pub(crate) fields: Map<String, Value>,
    pub(crate) answers: Vec<String>,
}

/// `/…/questions/<id>` with `answers` in place of `questions`; `None` when
/// `path` is not a question's path.
pub(crate) fn answer_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let [parents @ .., "questions", id] = &segments[..] else {
        return None;
    };
    if parents.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some(format!("/{}/answers/{id}", parents.join("/")))
}

/// The question at `path` and the answers that name it, refused with
/// `plans::<call>::no_question` when nothing there reads as a question.
pub(crate) fn read(host: &dyn Host, call: &str, path: &str) -> Result<Asked, PluginError> {
    let report = (host.query(&question_sql(path), Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    let rows = &report.rows;
    let no_question =
        |message: String, hint: &str| refused(call, "no_question", path, message, hint);
    let row = rows
        .iter()
        .find(|row| row.first().and_then(Value::as_str) == Some("question"));
    let Some([_, _, kind, Value::String(blob), body]) = row.map(Vec::as_slice) else {
        return Err(no_question(
            format!("nothing at {path} is a question"),
            "list the open questions with `board call plans.open`",
        ));
    };
    if kind != QuestionRecord::TYPE {
        return Err(no_question(
            format!(
                "{path} is a {}, not a {}",
                kind.as_str().unwrap_or("file without a type"),
                QuestionRecord::TYPE
            ),
            "list the open questions with `board call plans.open`",
        ));
    }
    let record = (body.as_str()).and_then(|body| parse_json(body.as_bytes()).ok());
    let Some(Value::Object(fields)) = record else {
        return Err(no_question(
            format!("{path} does not read as a {}", QuestionRecord::TYPE),
            "run `board check` on the path and fix the question",
        ));
    };
    let answers = (rows.iter())
        .filter(|row| row.first().and_then(Value::as_str) == Some("answer"))
        .filter_map(|row| row.get(1).and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    Ok(Asked {
        blob: blob.clone(),
        fields,
        answers,
    })
}

impl Asked {
    /// Refuses with `plans::<call>::already_answered` when an answer names
    /// the question, and with `plans::<call>::not_open` when its status is
    /// not `open`.
    pub(crate) fn still_open(&self, call: &str, path: &str) -> Result<(), PluginError> {
        if !self.answers.is_empty() {
            return Err(refused(
                call,
                "already_answered",
                path,
                format!(
                    "{path} already has an answer at {}, so nothing was recorded",
                    self.answers.join(", ")
                ),
                "read that answer with `board get`; a question has one answer",
            ));
        }
        let status = self.fields.get("status").cloned().unwrap_or(Value::Null);
        if status != "open" {
            let status = status
                .as_str()
                .map_or_else(|| status.to_string(), str::to_owned);
            return Err(refused(
                call,
                "not_open",
                path,
                format!("{path} is {status}, not open, so nothing was recorded"),
                "act on a question that `board call plans.open` lists",
            ));
        }
        Ok(())
    }
}

/// Refuses a `path` that is not `.../questions/<id>` with
/// `plans::<call>::not_a_question_path`, and gives its answer's path.
pub(crate) fn answer_path_of(call: &str, path: &str) -> Result<String, PluginError> {
    answer_path(path).ok_or_else(|| {
        refused(
            call,
            "not_a_question_path",
            path,
            format!("{path} is not a question's path, which ends in /questions/<id>"),
            "give the path plans.open lists for the question",
        )
    })
}

/// `plans::<call>::<reason>`, naming the question's path.
pub(crate) fn refused(
    call: &str,
    reason: &str,
    path: &str,
    message: String,
    hint: &str,
) -> PluginError {
    let mut fields = Map::new();
    fields.insert("path".to_owned(), json!(path));
    PluginError {
        code: format!("plans::{call}::{reason}"),
        message,
        fields,
        hint: Some(hint.to_owned()),
    }
}

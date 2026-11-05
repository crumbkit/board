//! `plans.answer`: records the owner's answer to an open question, as the
//! Claude Code mod's pane does, in Board's own process.

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError, literal};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{AnswerRecord, QuestionRecord};

/// The owner's answer to the question at `path`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AnswerInput")]
pub(crate) struct Input {
    /// The question's logical path, such as /projects/board/questions/20261024T1830Z-k3.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The id of the option the owner chose, or none when the owner's words choose no option.
    #[schemars(length(min = 1))]
    option: String,
    /// The owner's words, verbatim; the chosen option's text when left out, and needed when option is none.
    #[schemars(length(min = 1))]
    words: Option<String>,
    /// The session or client that writes the answer, such as a local_ session id.
    #[schemars(length(min = 1))]
    by: Option<String>,
    /// The client and surface that wrote the answer, such as board-app/phone.
    #[schemars(regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"))]
    written_by: Option<String>,
}

/// The answer as recorded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AnswerOutput")]
pub(crate) struct Output {
    /// The answer's path, the question's id under answers/.
    answer: String,
    /// The question's path, whose status is now answered.
    question: String,
    /// The chosen option's id, or none.
    choice: String,
    /// The words recorded.
    words: String,
    /// When the answer was recorded, in UTC to the second.
    #[schemars(extend("format" = "date-time"))]
    answered: String,
}

/// The question at `path` and every answer that names it, at any id.
pub(crate) fn question_sql(path: &str) -> String {
    let path = literal(path);
    format!(
        "SELECT 'question' AS kind, logical, type, blob, body FROM records \
         WHERE logical = {path} \
         UNION ALL SELECT 'answer', logical, type, blob, NULL FROM records \
         WHERE type = '{}' AND json_extract(body, '$.question') = {path}",
        AnswerRecord::TYPE
    )
}

pub(crate) fn answer(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        option,
        words,
        by,
        written_by,
    } = PluginError::read_input("plans", "answer", input)?;
    let Some(answer_path) = answer_path(&path) else {
        return Err(refused(
            "not_a_question_path",
            &path,
            format!("{path} is not a question's path, which ends in /questions/<id>"),
            "give the path plans.open lists for the question",
        ));
    };
    if option == "none" && words.is_none() {
        return Err(refused(
            "words_needed",
            &path,
            "option none chooses no option, so the answer needs the owner's words".to_owned(),
            "give words, or give the id of an option",
        ));
    }
    let report = (host.query(&question_sql(&path), Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    let (blob, mut asked) = question(&path, &report.rows)?;
    let answers: Vec<&str> = (report.rows.iter())
        .filter(|row| row.first().and_then(Value::as_str) == Some("answer"))
        .filter_map(|row| row.get(1).and_then(Value::as_str))
        .collect();
    if !answers.is_empty() {
        return Err(refused(
            "already_answered",
            &path,
            format!(
                "{path} already has an answer at {}, so nothing was recorded",
                answers.join(", ")
            ),
            "read that answer with `board get`; a question has one answer",
        ));
    }
    let status = asked.get("status").cloned().unwrap_or(Value::Null);
    if status != "open" {
        let status = status
            .as_str()
            .map_or_else(|| status.to_string(), str::to_owned);
        return Err(refused(
            "not_open",
            &path,
            format!("{path} is {status}, not open, so nothing was recorded"),
            "answer a question that `board call plans.open` lists",
        ));
    }
    let options = options(&asked);
    let chosen = options.iter().find(|(id, _)| *id == option);
    if option != "none" && chosen.is_none() {
        let ids: Vec<&str> = options.iter().map(|(id, _)| *id).collect();
        let ids = if ids.is_empty() {
            "no options".to_owned()
        } else {
            ids.join(", ")
        };
        return Err(refused(
            "unknown_option",
            &path,
            format!("{path} has no option {option}; it offers {ids}"),
            "give one of those ids, or none with the owner's words",
        ));
    }
    // `none` came with words, so only a chosen option's text fills a gap.
    let words = words
        .or_else(|| chosen.map(|(_, text)| (*text).to_owned()))
        .unwrap_or_default();
    let answered = humantime::format_rfc3339_seconds(host.settings().now).to_string();
    let mut record = Map::new();
    record.insert("$type".to_owned(), json!(AnswerRecord::TYPE));
    record.insert("question".to_owned(), json!(path));
    record.insert("choice".to_owned(), json!(option));
    record.insert("words".to_owned(), json!(words));
    record.insert("answered".to_owned(), json!(answered));
    if let Some(by) = by {
        record.insert("by".to_owned(), json!(by));
    }
    if let Some(written_by) = written_by {
        record.insert("written_by".to_owned(), json!(written_by));
    }
    asked.insert("status".to_owned(), json!("answered"));
    let lines = [
        json!({"path": answer_path, "expect": "none", "record": record}),
        json!({"path": path, "expect": blob, "record": asked}),
    ];
    (host.put(&lines))
        .map_err(|error| PluginError::board_refused("plans", "put --batch", &error.refusal()))?;
    let output = Output {
        answer: answer_path,
        question: path,
        choice: option,
        words,
        answered,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings"))
}

/// `/…/questions/<id>` with `answers` in place of `questions`; `None` when
/// `path` is not a question's path.
fn answer_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let [parents @ .., "questions", id] = &segments[..] else {
        return None;
    };
    if parents.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some(format!("/{}/answers/{id}", parents.join("/")))
}

/// The blob and fields of the question row in `rows`.
fn question(path: &str, rows: &[Vec<Value>]) -> Result<(String, Map<String, Value>), PluginError> {
    let row = rows
        .iter()
        .find(|row| row.first().and_then(Value::as_str) == Some("question"));
    let Some([_, _, kind, Value::String(blob), body]) = row.map(Vec::as_slice) else {
        return Err(refused(
            "no_question",
            path,
            format!("nothing at {path} is a question"),
            "list the open questions with `board call plans.open`",
        ));
    };
    if kind != QuestionRecord::TYPE {
        return Err(refused(
            "no_question",
            path,
            format!(
                "{path} is a {}, not a {}",
                kind.as_str().unwrap_or("file without a type"),
                QuestionRecord::TYPE
            ),
            "list the open questions with `board call plans.open`",
        ));
    }
    let record = (body.as_str()).and_then(|body| parse_json(body.as_bytes()).ok());
    let Some(Value::Object(record)) = record else {
        return Err(refused(
            "no_question",
            path,
            format!("{path} does not read as a {}", QuestionRecord::TYPE),
            "run `board check` on the path and fix the question",
        ));
    };
    Ok((blob.clone(), record))
}

/// The id and text of each of the question's options that has both.
fn options(question: &Map<String, Value>) -> Vec<(&str, &str)> {
    let options = question.get("options").and_then(Value::as_array);
    (options.into_iter().flatten())
        .filter_map(|option| {
            let id = option.get("id")?.as_str()?;
            Some((id, option.get("text")?.as_str()?))
        })
        .collect()
}

/// `plans::answer::<reason>`, naming the question's path.
fn refused(reason: &str, path: &str, message: String, hint: &str) -> PluginError {
    let mut fields = Map::new();
    fields.insert("path".to_owned(), json!(path));
    PluginError {
        code: format!("plans::answer::{reason}"),
        message,
        fields,
        hint: Some(hint.to_owned()),
    }
}

#[cfg(test)]
mod tests;

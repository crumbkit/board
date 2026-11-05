//! `plans.answer`: records the owner's answer to an open question, as the
//! Claude Code mod's pane does, in Board's own process. The answer is what
//! makes the question answered; the question's status stays `open`.

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[cfg(test)]
pub(crate) use crate::question::question_sql;
use crate::question::{self, answer_path_of, refused};
use crate::records::AnswerRecord;

/// The owner's answer to the question at `path`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AnswerInput")]
pub(crate) struct Input {
    /// The question's logical path, such as /projects/board/questions/20261024T1830Z-k3.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The id of the option the owner chose, or none when the owner's words choose no option; a question with an option whose id is none chooses that option.
    #[schemars(length(min = 1))]
    option: String,
    /// The owner's words, verbatim; the chosen option's text when left out, and needed when option none chooses no option.
    #[schemars(length(min = 1))]
    words: Option<String>,
    /// The session or client that writes the answer, such as a local_ session id.
    #[schemars(length(min = 1))]
    by: Option<String>,
    /// The plugin and hook, or the client and surface, that wrote the answer, such as plans/pane-answer or board-app/phone, when the owner answered through it rather than to an agent.
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
    /// The question's path, which the answer now names.
    question: String,
    /// The chosen option's id, or none.
    choice: String,
    /// The words recorded.
    words: String,
    /// When the answer was recorded, in UTC to the second.
    #[schemars(extend("format" = "date-time"))]
    answered: String,
}

pub(crate) fn answer(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        option,
        words,
        by,
        written_by,
    } = PluginError::read_input("plans", "answer", input)?;
    let answer_path = answer_path_of("answer", &path)?;
    let asked = question::read(host, "answer", &path)?;
    asked.still_open("answer", &path)?;
    let options = options(&asked.fields);
    let chosen = options.iter().find(|(id, _)| *id == option);
    if option == "none" && chosen.is_none() && words.is_none() {
        return Err(refused(
            "answer",
            "words_needed",
            &path,
            "option none chooses no option, so the answer needs the owner's words".to_owned(),
            "give words, or give the id of an option",
        ));
    }
    if option != "none" && chosen.is_none() {
        let ids: Vec<&str> = options.iter().map(|(id, _)| *id).collect();
        let ids = if ids.is_empty() {
            "no options".to_owned()
        } else {
            ids.join(", ")
        };
        return Err(refused(
            "answer",
            "unknown_option",
            &path,
            format!("{path} has no option {option}; it offers {ids}"),
            "give one of those ids, or none with the owner's words",
        ));
    }
    // Only a chosen option's text fills a gap: `none` that chooses no option
    // came with words.
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
    // The question is put back unchanged, so that a question withdrawn or
    // edited after the read refuses the whole batch.
    let lines = [
        json!({"path": answer_path, "expect": "none", "record": record}),
        json!({"path": path, "expect": asked.blob, "record": asked.fields}),
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

#[cfg(test)]
mod tests;

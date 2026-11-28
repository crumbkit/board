//! `plans.answer`: records the owner's answer to an open question, in Board's
//! own process. Every client answers through it, the Claude Code mod's pane
//! included. The answer is what makes the question answered; the question's
//! status stays `open`.

use board_ops::{Host, PluginError};
use board_plugin::Target;
use board_plugin::shape::{Actor, Person, Prose, Record, SessionId, Slug};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[cfg(test)]
pub(crate) use crate::question::question_sql;
use crate::question::{self, answer_path_of, refused};
use crate::records::{AnswerRecord, CHANGE_TYPES};

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
    /// The session that writes the answer, by its local_ id.
    #[schemars(with = "Option<SessionId>")]
    session: Option<String>,
    /// The program that writes the answer, such as board-app.
    #[schemars(with = "Option<Slug>")]
    program: Option<String>,
    /// The person who writes the answer, such as ana.
    #[schemars(with = "Option<Person>")]
    person: Option<String>,
    /// The name the agent that writes the answer was given.
    #[schemars(with = "Option<Prose>")]
    agent: Option<String>,
    /// The plugin and hook, or the client and surface, that wrote the answer, such as plans/pane-answer or board-app/phone, when the owner answered through it rather than to an agent.
    #[schemars(regex(pattern = r"^[a-z0-9-]+/[a-z0-9-]+$"))]
    written_by: Option<String>,
}

/// The answer as recorded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AnswerOutput")]
pub(crate) struct Output {
    /// `already_answered` when the question already had this answer, with
    /// the same option and words, so nothing was written and the other
    /// fields are the stored answer's; left out when the answer was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<Outcome>,
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
    /// The change the question blocks, which `changes.attach` was asked to
    /// keep the answer; left out when the question blocks no change.
    #[serde(skip_serializing_if = "Option::is_none")]
    attached: Option<Attached>,
    /// One line that says the question already had this answer; left out
    /// when the answer was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
}

/// What a call that wrote nothing found.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    /// The question already had the same answer.
    AlreadyAnswered,
}

/// The change the answer was attached to. A refused attach leaves the
/// answer recorded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Attached {
    /// The change's logical path, such as /changes/fix-the-sync.
    change: String,
    /// The code `changes.attach` refused with; left out when it attached
    /// the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<String>,
}

pub(crate) fn answer(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        option,
        words,
        session,
        program,
        person,
        agent,
        written_by,
    } = PluginError::read_input("plans", "answer", input)?;
    let by = Actor::of(session, program, person, agent);
    let answer_path = answer_path_of("answer", &path)?;
    let asked = question::read(host, "answer", &path)?;
    let options = options(&asked.fields);
    let chosen = options.iter().find(|(id, _)| *id == option);
    // A repeat of the stored answer, as after a retry, succeeds and writes
    // nothing; any other answer to an answered question is refused.
    let given = (words.clone()).or_else(|| chosen.map(|(_, text)| (*text).to_owned()));
    if let Some((answer, stored)) =
        (given.as_deref()).and_then(|given| asked.same_answer(&option, given))
    {
        return Ok(already_answered(path, answer, stored));
    }
    asked.still_open("answer", &path)?;
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
    let answered = board_model::utc_seconds(host.settings().now);
    let mut record = Map::new();
    record.insert("$type".to_owned(), json!(AnswerRecord::TYPE));
    // A reference is read from the root of the board it is in, and the
    // answer lands in the question's board.
    record.insert("question".to_owned(), json!(asked.own));
    // `none` that chooses no option leaves `choice` out.
    if chosen.is_some() {
        record.insert("choice".to_owned(), json!(option));
    }
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
    // `changes.attach` reads a change's plan from the root board, so only a
    // question in the root board, whose own path is its logical one, attaches.
    let work = asked.work.filter(|_| asked.own == path);
    let attached = attach(host, work, &answer_path);
    let output = Output {
        outcome: None,
        answer: answer_path,
        question: path,
        choice: option,
        words,
        answered,
        attached,
        line: None,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings"))
}

/// The stored answer at `answer` to the question at `path`, which a repeat
/// of it found, with the line that says so.
fn already_answered(path: String, answer: &str, stored: &Map<String, Value>) -> Value {
    let text = |key: &str| (stored.get(key).and_then(Value::as_str)).map(str::to_owned);
    let output = Output {
        outcome: Some(Outcome::AlreadyAnswered),
        line: Some(format!(
            "already answered: {path} has this answer at {answer}"
        )),
        answer: answer.to_owned(),
        question: path,
        choice: text("choice").unwrap_or_else(|| "none".to_owned()),
        words: text("words").unwrap_or_default(),
        answered: text("answered").unwrap_or_default(),
        attached: None,
    };
    serde_json::to_value(output).expect("Output holds only strings")
}

/// Asks `changes.attach` to keep `answer` on the plan of the change the
/// question blocks, so that the change's record holds the owner's answer.
/// `None` when `work` is no change. A task's record is Markdown in a notes
/// repository, which has no list of references to add the answer to.
fn attach(host: &dyn Host, work: Option<(String, String)>, answer: &str) -> Option<Attached> {
    let (change, kind) = work?;
    if !CHANGE_TYPES.contains(&kind.as_str()) {
        return None;
    }
    let target: Target = "changes.attach"
        .parse()
        .expect("changes.attach is <plugin>.<capability>");
    let refused = (host
        .call(&target, &json!({"path": change, "ref": answer}))
        .err())
    .map(|refusal| refusal.code.as_str().to_owned());
    Some(Attached { change, refused })
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

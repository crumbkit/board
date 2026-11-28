//! The line `board get` prints on stderr beside an open question: the
//! answer at the question's id under answers/, or that none is there
//! (docs/architecture/records/get.md, Notes beside a record). A question's
//! stored status stays `open` once it is answered, so this line is how a
//! reader of `board get` learns of the answer.

use board_model::LogicalPath;
use board_ops::{Asked, Beside, GetNote};
use board_plugin::shape::Record;
use serde_json::Value;

use crate::question::answer_path;
use crate::records::{AnswerRecord, QuestionRecord};

/// How many characters of the owner's words the line quotes.
const WORDS: usize = 80;

pub(crate) static GET_NOTES: [GetNote; 1] = [GetNote {
    record_type: QuestionRecord::TYPE,
    beside,
    line,
}];

/// `/…/answers/<id>` beside an open question at `/…/questions/<id>`, under
/// the same mount; nothing beside a question in any other status.
fn beside(asked: &Asked) -> Option<LogicalPath> {
    if asked.record.get("status").and_then(Value::as_str) != Some("open") {
        return None;
    }
    answer_path(asked.display.as_str())?.parse().ok()
}

/// The answer beside the question, when it names the question by the path
/// the reader asked, by its path once pointers are followed, or by its path
/// in the board that holds both; otherwise that no answer is there.
#[expect(
    clippy::unnecessary_wraps,
    reason = "GetNote::line returns an Option, so that a note can print nothing; a question's always prints"
)]
fn line(asked: &Asked, beside: &Beside<'_>) -> Option<String> {
    let question = &asked.path;
    let (path, record) = match beside {
        Beside::Failed { code, .. } => return Some(format!("{question}: answer not read: {code}")),
        Beside::Nothing { path } => (*path, None),
        Beside::Record { path, record } => (*path, Some(*record)),
    };
    let Some(answer) = record.filter(|record| answers(record, asked)) else {
        return Some(format!(
            "{question}: no answer at {path}; run board refs {question} to find an answer at another path"
        ));
    };
    let text = |field: &str| answer.get(field).and_then(Value::as_str);
    let words: String = (text("words").unwrap_or_default().chars())
        .take(WORDS)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    Some(format!(
        "{question}: answered by {path} ({}): {}, \"{words}\"",
        text("answered").unwrap_or_default(),
        text("choice").unwrap_or("none"),
    ))
}

/// Whether `record` is an answer that names `asked`.
fn answers(record: &Value, asked: &Asked) -> bool {
    let named = |field: &str| record.get(field).and_then(Value::as_str);
    named("$type") == Some(AnswerRecord::TYPE)
        && named("question").is_some_and(|question| {
            [asked.path.as_str(), asked.display.as_str(), &asked.held].contains(&question)
        })
}

#[cfg(test)]
mod tests;

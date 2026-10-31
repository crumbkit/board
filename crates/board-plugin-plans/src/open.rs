//! `plans.open`: the questions still waiting for the owner, oldest first.

mod asked;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::{TREE_CODES, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use self::asked::{Instant, instant};
use crate::records::Choice;

/// Every open question. The path's prefix is matched in Rust: `starts_with`
/// compares as `substr` and `length` would in `SQLite`.
pub(crate) const QUESTIONS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'plans/question/v1' AND json_extract(body, '$.status') = 'open'";

/// The open questions under `path`, oldest first.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "OpenInput")]
pub(crate) struct Input {
    /// Only questions below this logical path, such as /projects/board;
    /// every question when left out.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The most questions to return, 1 to 10,000; 100 when left out.
    #[serde(deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10_000))]
    limit: u16,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            path: "/".to_owned(),
            limit: 100,
        }
    }
}

/// Open questions, oldest first.
///
/// `total` counts every readable open question under the path and
/// `truncated` says whether the limit left some out. `unreadable` is how
/// many open questions were left out because Board marks them degraded or
/// this plugin cannot read them.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OpenOutput")]
pub(crate) struct Output {
    questions: Vec<Waiting>,
    total: usize,
    truncated: bool,
    unreadable: usize,
}

/// An open question, by path, with what the owner needs to answer it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OpenQuestion")]
struct Waiting {
    path: String,
    question: String,
    #[schemars(extend("format" = "date-time"))]
    asked: String,
    session: String,
    options: Vec<Choice>,
    recommendation: Option<String>,
    blocks: Option<String>,
}

/// Options are read as objects because serde reads a struct from a list too.
#[derive(Deserialize)]
struct Stored {
    question: String,
    session: String,
    asked: String,
    #[serde(default)]
    options: Vec<Map<String, Value>>,
    recommendation: Option<String>,
    blocks: Option<String>,
}

pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, limit } = PluginError::read_input("plans", "open", input)?;
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let limit = usize::from(limit);
    let report = (host.query(QUESTIONS, Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    let mut found: Vec<(Instant, Waiting)> = Vec::new();
    let mut unreadable = 0;
    for row in &report.rows {
        let [Value::String(logical), code, body] = &row[..] else {
            continue;
        };
        if !logical.starts_with(&prefix) {
            continue;
        }
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        match tree.then(|| readable(logical, body)).flatten() {
            Some(read) => found.push(read),
            None => unreadable += 1,
        }
    }
    found.sort_by(|(a, first), (b, second)| (a, &first.path).cmp(&(b, &second.path)));
    let total = found.len();
    let output = Output {
        questions: found
            .into_iter()
            .take(limit)
            .map(|(_, read)| read)
            .collect(),
        total,
        truncated: total > limit,
        unreadable,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// `None` when `body` lacks the shape `open` returns, as a mounted board's may.
fn readable(path: &str, body: &Value) -> Option<(Instant, Waiting)> {
    let record = parse_json(body.as_str()?.as_bytes())
        .ok()
        .filter(Value::is_object)?;
    let asked = Stored::deserialize(&record).ok()?;
    let when = instant(&asked.asked)?;
    let options = asked.options.iter().map(choice).collect::<Option<_>>()?;
    let question = Waiting {
        path: path.to_owned(),
        question: asked.question,
        asked: asked.asked,
        session: asked.session,
        options,
        recommendation: asked.recommendation,
        blocks: asked.blocks,
    };
    Some((when, question))
}

fn choice(option: &Map<String, Value>) -> Option<Choice> {
    let text = |name| {
        (option.get(name)?.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let (id, text) = (text("id")?, text("text")?);
    (option.len() == 2).then_some(Choice { id, text })
}

#[cfg(test)]
mod tests;

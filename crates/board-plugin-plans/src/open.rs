//! `plans.open`: the questions still waiting for the owner, oldest first.

mod asked;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use self::asked::{Instant, instant};

/// Every open question. The path's prefix is matched in Rust: `starts_with`
/// compares as `substr` and `length` would in `SQLite`.
const QUESTIONS: &str = "SELECT logical, code, body FROM records \
     WHERE type = 'plans/question/v1' AND json_extract(body, '$.status') = 'open'";

/// Codes that depend on the tree rather than the record. A question
/// holding one is still a question to read.
const TREE_CODES: [&str; 2] = ["board::ref::dangling", "board::ref::wrong_type"];

/// The open questions under `path`, oldest first.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OpenInput")]
pub(crate) struct Input {
    /// Only questions below this logical path, such as /projects/board;
    /// every question when left out.
    #[serde(default = "root")]
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// The most questions to return, 1 to 10,000; 100 when left out.
    #[serde(default = "limit", deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10_000))]
    limit: u16,
}

fn root() -> String {
    "/".to_owned()
}

/// The default `limit` (STANDARDS.md 23).
fn limit() -> u16 {
    100
}

/// A whole number the input schema admitted, such as `2` or `2.0`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the input schema admits only whole numbers from 1 to 10,000"
)]
fn whole<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    f64::deserialize(deserializer).map(|number| number as u16)
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

/// One answer the owner may give. An answer's `choice` and the question's
/// `recommendation` name it by `id`.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Option")]
struct Choice {
    #[schemars(length(min = 1))]
    id: String,
    #[schemars(length(min = 1))]
    text: String,
}

/// The fields of a `plans/question/v1` that `open` returns. Options left
/// out are none; options of `null` are not readable. Each option is read
/// as an object, since serde would read a struct from a list too.
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

/// `T`'s JSON Schema, draft 2020-12 without `$schema`, as a manifest holds
/// it. An integer's `format`, such as `uint16`, says only how Rust stores it.
fn schema<T: JsonSchema>(settings: SchemaSettings) -> Value {
    let mut settings =
        settings.with_transform(RecursiveTransform(|schema: &mut schemars::Schema| {
            if schema.get("type").and_then(Value::as_str) == Some("integer") {
                schema.remove("format");
            }
        }));
    settings.meta_schema = None;
    settings.into_generator().into_root_schema_for::<T>().into()
}

/// The manifest's `inputSchema` for `open`.
pub(crate) fn input_schema() -> Value {
    schema::<Input>(SchemaSettings::draft2020_12())
}

/// The manifest's `outputSchema` for `open`.
pub(crate) fn output_schema() -> Value {
    schema::<Output>(SchemaSettings::draft2020_12().for_serialize())
}

/// Every question under `input`'s `path` whose `status` is `open`, oldest
/// first, at most `limit`. Board has checked `input` against the input
/// schema.
pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, limit } = Input::deserialize(input).map_err(|error| PluginError {
        code: "plans::call::bad_input".to_owned(),
        message: format!("open's input {input} does not read: {error}"),
        fields: Map::new(),
    })?;
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
    Ok(serde_json::to_value(output).unwrap_or_else(|_| Value::Object(Map::new())))
}

/// The question in `body` and when it was asked, or `None` when it does not
/// have the shape `open` returns. Board has validated the record, but a
/// mounted board validates against its own schemas.
fn readable(path: &str, body: &Value) -> Option<(Instant, Waiting)> {
    let record = parse_json(body.as_str()?.as_bytes()).ok()?;
    if !record.is_object() {
        return None;
    }
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

/// An option holding exactly `id` and `text`, both non-empty strings.
fn choice(option: &Map<String, Value>) -> Option<Choice> {
    let text = |name: &str| {
        let text = option.get(name)?.as_str()?;
        (!text.is_empty()).then(|| text.to_owned())
    };
    (option.len() == 2).then_some(())?;
    Some(Choice {
        id: text("id")?,
        text: text("text")?,
    })
}

#[cfg(test)]
mod tests;

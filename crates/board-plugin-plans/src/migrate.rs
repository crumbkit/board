//! `plans.migrate`: rewrites each question of this board whose `blocks` still
//! holds words, as it did before `blocks` became a reference to a change.
//!
//! A value that is a change's path is already a reference and stays. A value
//! that names exactly one change becomes that change's path, and its words
//! move to `blocked_work`. Any other value moves to `blocked_work`. A value
//! that is no string, or a question that already holds `blocked_work`, is
//! listed under `unread` and left. Only this board's own questions are read,
//! because a nested board may reference only itself.

use std::collections::BTreeSet;

use board_model::parse_json;
use board_ops::{Bulk, Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, schema_of};
use board_validate::Schema;
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{CHANGE_TYPES, QuestionRecord};

/// The reason the migration's commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "migrate questions";
/// The most rewritten paths the output lists (STANDARDS 23).
const LISTED: usize = 100;
/// The code of a question the migration cannot rewrite.
const UNREAD: &str = "plans::migrate::unread";

/// Whether to write the migration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlansMigrateInput")]
pub(crate) struct Input {
    /// Write every rewritten question; without it nothing is written.
    #[serde(default)]
    apply: bool,
}

/// What the migration rewrote or would rewrite.
///
/// `rewritten` lists the questions, at most 100, with `total` and
/// `truncated`. `referenced` counts those whose `blocks` became a reference.
/// `unread` lists each question left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PlansMigrateOutput")]
pub(crate) struct Output {
    applied: bool,
    rewritten: Vec<String>,
    total: usize,
    truncated: bool,
    referenced: usize,
    unread: Vec<Unread>,
}

/// A question left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
struct Unread {
    path: String,
    code: &'static str,
    why: String,
}

/// What becomes of one question.
#[derive(Debug, PartialEq)]
enum Outcome {
    Keep,
    Rewrite {
        body: Map<String, Value>,
        referenced: bool,
    },
    Unread(String),
}

/// The rows of `sql`, or the refusal of a query Board cut short.
fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    if report.truncated {
        return Err(PluginError {
            code: "plans::migrate::over_budget".to_owned(),
            message: "the board's questions passed the output budget of one board run; migrate \
                      a smaller board, or raise the budget"
                .to_owned(),
            fields: Map::new(),
            hint: None,
        });
    }
    Ok(report.rows)
}

/// The path of each change in this board's own tree.
fn changes(host: &dyn Host) -> Result<BTreeSet<String>, PluginError> {
    let types: Vec<String> = CHANGE_TYPES.iter().map(|each| literal(each)).collect();
    let sql = format!(
        "SELECT logical FROM entries WHERE source = 'worktree' AND type IN ({})",
        types.join(", ")
    );
    Ok((rows(host, &sql)?.iter())
        .filter_map(|row| Some(row.first()?.as_str()?.to_owned()))
        .collect())
}

/// The changes of `changes` that `text` names, each once.
fn named<'a>(text: &str, changes: &'a BTreeSet<String>) -> BTreeSet<&'a str> {
    (text.split(|each: char| each.is_whitespace() || "`'\"()<>[],;".contains(each)))
        .map(|word| word.trim_end_matches(['.', ':']))
        .filter_map(|word| changes.get(word).map(String::as_str))
        .collect()
}

/// What becomes of the question whose fields are `body`.
fn outcome(mut body: Map<String, Value>, changes: &BTreeSet<String>) -> Outcome {
    let text = match body.get("blocks") {
        None | Some(Value::Null) => return Outcome::Keep,
        Some(Value::String(text)) if changes.contains(text) => return Outcome::Keep,
        Some(Value::String(text)) => text.clone(),
        Some(other) => {
            return Outcome::Unread(format!(
                "its blocks is {other}, which is no text; rewrite it by hand"
            ));
        }
    };
    if body.get("blocked_work").is_some_and(|held| !held.is_null()) {
        return Outcome::Unread(
            "it holds both words in blocks and blocked_work; merge them by hand".to_owned(),
        );
    }
    let found = named(&text, changes);
    let referenced = found.len() == 1;
    body.remove("blocks");
    if let (true, Some(change)) = (referenced, found.first()) {
        body.insert("blocks".to_owned(), json!(change));
    }
    body.insert("blocked_work".to_owned(), json!(text));
    Outcome::Rewrite { body, referenced }
}

pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { apply } = PluginError::read_input("plans", "migrate", input)?;
    let schema = Schema::compile(schema_of::<QuestionRecord>(SchemaSettings::draft2020_12()))
        .expect("the question's schema is generated from its struct; a test compiles it");
    let changes = changes(host)?;
    let sql = format!(
        "SELECT e.logical, e.blob, b.body FROM entries AS e JOIN bodies AS b ON b.blob = e.blob \
         WHERE e.type = {} AND e.source = 'worktree' ORDER BY e.logical",
        literal(QuestionRecord::TYPE)
    );
    let (mut rewritten, mut lines, mut unread) = (Vec::new(), Vec::new(), Vec::new());
    let mut referenced_count = 0;
    for row in rows(host, &sql)? {
        let [
            Value::String(path),
            Value::String(blob),
            Value::String(text),
        ] = &row[..]
        else {
            continue;
        };
        let leave = |why: String| Unread {
            path: path.clone(),
            code: UNREAD,
            why,
        };
        let Ok(Value::Object(body)) = parse_json(text.as_bytes()) else {
            unread.push(leave("it does not read as a JSON object".to_owned()));
            continue;
        };
        match outcome(body, &changes) {
            Outcome::Keep => {}
            Outcome::Unread(why) => unread.push(leave(why)),
            Outcome::Rewrite { body, referenced } => {
                let record = Value::Object(body);
                if let Err(error) = schema.validate(&record) {
                    unread.push(leave(format!(
                        "its rewritten record {error}; fix the question and run migrate again"
                    )));
                    continue;
                }
                referenced_count += usize::from(referenced);
                rewritten.push(path.clone());
                lines.push(json!({"path": path, "expect": blob, "record": record}));
            }
        }
    }
    if apply && !lines.is_empty() {
        let refused = |refusal: &board_model::Refusal| {
            PluginError::board_refused("plans", "put --batch --bulk", refusal)
        };
        let bulk = Bulk::new(BULK).map_err(|error| refused(&error.refusal()))?;
        (host.put_bulk(&lines, &bulk)).map_err(|error| refused(&error.refusal()))?;
    }
    let total = rewritten.len();
    rewritten.truncate(LISTED);
    let output = Output {
        applied: apply,
        rewritten,
        total,
        truncated: total > LISTED,
        referenced: referenced_count,
        unread,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

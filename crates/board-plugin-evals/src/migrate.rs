//! `evals.migrate`: rewrites each eval of this board whose `decision` still
//! holds words, as it did before `decision` became a reference.
//!
//! The words move to `informs`. When exactly one decision record of this
//! board names the eval in its `refs`, `decision` references it; when several
//! do, they are listed and `decision` is left out. A `decision` that already
//! names a decision record stays. A value that is no string, or an eval that
//! already holds `informs`, is listed under `unread` and left.

use std::collections::{BTreeMap, BTreeSet};

use board_model::parse_json;
use board_ops::{Bulk, Host, Limit, PluginError, literal};
use board_plugin::shape::{Record, schema_of};
use board_validate::Schema;
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{Decision, EvalRecord};

/// The reason the migration's commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "migrate evals";
/// The most rewritten paths the output lists (STANDARDS 23).
const LISTED: usize = 100;
/// The code of an eval the migration cannot rewrite.
const UNREAD: &str = "evals::migrate::unread";

/// Whether to write the migration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "EvalsMigrateInput")]
pub(crate) struct Input {
    /// Write every rewritten eval; without it nothing is written.
    #[serde(default)]
    apply: bool,
}

/// What the migration rewrote or would rewrite.
///
/// `rewritten` lists the evals, at most 100, with `total` and `truncated`.
/// `referenced` counts those whose `decision` became a reference. `several`
/// lists each eval that more than one decision names, and `unread` each eval
/// left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "EvalsMigrateOutput")]
pub(crate) struct Output {
    applied: bool,
    rewritten: Vec<String>,
    total: usize,
    truncated: bool,
    referenced: usize,
    several: Vec<Several>,
    unread: Vec<Unread>,
}

/// An eval that several decisions name, which keeps no `decision`.
#[derive(Debug, Serialize, JsonSchema)]
struct Several {
    path: String,
    decisions: Vec<String>,
}

/// An eval left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
struct Unread {
    path: String,
    code: &'static str,
    why: String,
}

/// The rows of `sql`, or the refusal of a query Board cut short.
fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("evals", "query", &error.refusal()))?;
    if report.truncated {
        return Err(PluginError {
            code: "evals::migrate::over_budget".to_owned(),
            message: "the board's evals passed the output budget of one board run; migrate a \
                      smaller board, or raise the budget"
                .to_owned(),
            fields: Map::new(),
            hint: None,
        });
    }
    Ok(report.rows)
}

/// The rows of this board's own records of `kind`, by path: path, blob and
/// body, whether Board reads them or not.
fn own(host: &dyn Host, kind: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    rows(
        host,
        &format!(
            "SELECT e.logical, e.blob, b.body FROM entries AS e JOIN bodies AS b ON b.blob = \
             e.blob WHERE e.type = {} AND e.source = 'worktree' ORDER BY e.logical",
            literal(kind)
        ),
    )
}

/// By each path a decision names in its `refs`, the decisions that name it.
type Naming = BTreeMap<String, Vec<String>>;

/// Each decision of this board, and what the decisions name.
fn decisions(host: &dyn Host) -> Result<(BTreeSet<String>, Naming), PluginError> {
    let (mut all, mut naming) = (BTreeSet::new(), Naming::new());
    for row in own(host, Decision::TYPE)? {
        let [Value::String(path), _, Value::String(text)] = &row[..] else {
            continue;
        };
        all.insert(path.clone());
        let Ok(body) = parse_json(text.as_bytes()) else {
            continue;
        };
        for named in (body
            .get("refs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(Value::as_str)
        {
            naming
                .entry(named.to_owned())
                .or_default()
                .push(path.clone());
        }
    }
    Ok((all, naming))
}

pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { apply } = PluginError::read_input("evals", "migrate", input)?;
    let schema = Schema::compile(schema_of::<EvalRecord>(SchemaSettings::draft2020_12()))
        .expect("the eval's schema is generated from its struct; a test compiles it");
    let (all, naming) = decisions(host)?;
    let (mut rewritten, mut lines, mut several, mut unread) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut referenced = 0;
    for row in own(host, EvalRecord::TYPE)? {
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
        let Ok(Value::Object(mut body)) = parse_json(text.as_bytes()) else {
            unread.push(leave("it does not read as a JSON object".to_owned()));
            continue;
        };
        let words = match body.get("decision") {
            None | Some(Value::Null) => continue,
            Some(Value::String(named)) if all.contains(named) => continue,
            Some(Value::String(words)) => words.clone(),
            Some(other) => {
                unread.push(leave(format!(
                    "its decision is {other}, which is no text; rewrite it by hand"
                )));
                continue;
            }
        };
        if body.get("informs").is_some_and(|held| !held.is_null()) {
            unread.push(leave(
                "it holds both words in decision and informs; merge them by hand".to_owned(),
            ));
            continue;
        }
        body.remove("decision");
        body.insert("informs".to_owned(), json!(words));
        let found = naming.get(path).map_or(&[][..], Vec::as_slice);
        if let [only] = found {
            body.insert("decision".to_owned(), json!(only));
        }
        let record = Value::Object(body);
        if let Err(error) = schema.validate(&record) {
            unread.push(leave(format!(
                "its rewritten record {error}; fix the eval and run migrate again"
            )));
            continue;
        }
        if found.len() > 1 {
            several.push(Several {
                path: path.clone(),
                decisions: found.to_vec(),
            });
        }
        referenced += usize::from(found.len() == 1);
        rewritten.push(path.clone());
        lines.push(json!({"path": path, "expect": blob, "record": record}));
    }
    if apply && !lines.is_empty() {
        let refused = |refusal: &board_model::Refusal| {
            PluginError::board_refused("evals", "put --batch --bulk", refusal)
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
        referenced,
        several,
        unread,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

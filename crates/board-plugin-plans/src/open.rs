//! `plans.open`: the questions still waiting for the owner, grouped by
//! project with the questions that block active work first. A question
//! waits while its status is open and no answer names it.

mod asked;

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::{Actor, TREE_CODES, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use self::asked::{Instant, instant};
use crate::question::{NAMED, OWN_PATH};
use crate::records::{CHANGE_TYPES, Choice, TASK_TYPE};

/// Every question whose status is open and that no answer names. The path's
/// prefix is matched in Rust: `starts_with` compares as `substr` and `length`
/// would in `SQLite`.
///
/// Each answer's `question` is read once into `named`, which `SQLite` keeps
/// and indexes by that value, so each question is looked up by key. Matching
/// every question against every answer's body would parse each body again
/// for each question and run past the one second a query may take.
///
/// The cast gives `target` the text affinity of `logical`. Without it,
/// `SQLite` applies that affinity to `target` when it compares the two, cannot
/// use an index of `target`, and scans `named` for every question.
///
/// The fourth column, `work`, says whether the change or task the question
/// `blocks` is still going: `done` or `dropped` once a change's phase or a
/// task's status is one of those, `active` before, and null when `blocks`
/// is absent, names no change or task, or names one whose phase or status
/// is null, as it is in a degraded record that leaves the field out. A question's `blocks` is read from the root of
/// its own board, so the work's logical path is the question's mount prefix
/// followed by `blocks`; the lookup is one equality on `logical`.
pub(crate) fn questions() -> String {
    let changes = CHANGE_TYPES.map(|each| format!("'{each}'")).join(", ");
    format!(
        "WITH named AS MATERIALIZED (SELECT a.source AS source, CAST(json_extract(a.body, '$.question') AS TEXT) AS target \
         FROM records a WHERE a.type = 'plans/answer/v1') \
         SELECT q.logical, q.code, q.body, \
         (SELECT CASE \
           WHEN w.type IN ({changes}) AND json_extract(w.body, '$.phase') IS NOT NULL THEN \
             CASE json_extract(w.body, '$.phase') WHEN 'done' THEN 'done' WHEN 'dropped' THEN 'dropped' \
             ELSE 'active' END \
           WHEN w.type = '{TASK_TYPE}' AND json_extract(w.body, '$.status') IS NOT NULL THEN \
             CASE json_extract(w.body, '$.status') WHEN 'done' THEN 'done' WHEN 'dropped' THEN 'dropped' \
             ELSE 'active' END \
           END \
          FROM records w \
          WHERE w.logical = substr(q.logical, 1, length(q.logical) - length({OWN_PATH})) \
            || json_extract(q.body, '$.blocks')) AS work \
         FROM records q \
         WHERE q.type = 'plans/question/v1' AND json_extract(q.body, '$.status') = 'open' \
         AND NOT {NAMED}"
    )
}

/// The open questions under `path`, in digest order.
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

/// Open questions in digest order: grouped by project, a project holding a
/// critical question first, and within a project the critical questions
/// first, each oldest first.
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
pub(crate) struct Waiting {
    path: String,
    question: String,
    #[schemars(extend("format" = "date-time"))]
    asked: String,
    session: String,
    options: Vec<Choice>,
    recommendation: Option<String>,
    why: Option<String>,
    blocks: Option<String>,
    blocked_work: Option<String>,
    note: Option<String>,
    mount: Option<String>,
    context_path: Option<String>,
    /// The folder that holds the question's `questions/` folder, such as
    /// `/projects/board`; the question's own folder when it is not in one.
    project: String,
    /// True when `blocks` names a change that is not done or dropped, or a
    /// task that is open.
    pub(crate) critical: bool,
}

/// Options are read as objects because serde reads a struct from a list too.
#[derive(Deserialize)]
struct Stored {
    question: String,
    by: Actor,
    asked: String,
    #[serde(default)]
    options: Vec<Map<String, Value>>,
    recommendation: Option<String>,
    why: Option<String>,
    blocks: Option<String>,
    blocked_work: Option<String>,
    note: Option<String>,
    mount: Option<String>,
    context_path: Option<String>,
}

pub(crate) fn open(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, limit } = PluginError::read_input("plans", "open", input)?;
    let limit = usize::from(limit);
    let (found, unreadable) = waiting(host, &path)?;
    let total = found.len();
    let output = Output {
        questions: found.into_iter().take(limit).collect(),
        total,
        truncated: total > limit,
        unreadable,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// Every readable open question under `path` in digest order, and how many
/// open questions there could not be read.
pub(crate) fn waiting(host: &dyn Host, path: &str) -> Result<(Vec<Waiting>, usize), PluginError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let report = (host.query(&questions(), Limit::ALL))
        .map_err(|error| PluginError::board_refused("plans", "query", &error.refusal()))?;
    let mut found: Vec<(Instant, Waiting)> = Vec::new();
    let mut unreadable = 0;
    for row in &report.rows {
        let [Value::String(logical), code, body, work] = &row[..] else {
            continue;
        };
        if !logical.starts_with(&prefix) {
            continue;
        }
        let tree = code.is_null() || TREE_CODES.iter().any(|tree| code == *tree);
        match tree
            .then(|| readable(logical, body, work == "active"))
            .flatten()
        {
            Some(read) => found.push(read),
            None => unreadable += 1,
        }
    }
    Ok((digest(found), unreadable))
}

/// Sorts questions into digest order. Within a project the critical ones
/// come first, then each oldest first, then by path. Projects come in the
/// order of their first question by the same key, so a project holding a
/// critical question comes before one holding none.
fn digest(mut found: Vec<(Instant, Waiting)>) -> Vec<Waiting> {
    found.sort_by(|(a_when, a), (b_when, b)| {
        (!a.critical, a_when, &a.path).cmp(&(!b.critical, b_when, &b.path))
    });
    let mut projects: Vec<(String, Vec<(Instant, Waiting)>)> = Vec::new();
    for each in found {
        match projects
            .iter_mut()
            .find(|(project, _)| *project == each.1.project)
        {
            Some((_, questions)) => questions.push(each),
            None => projects.push((each.1.project.clone(), vec![each])),
        }
    }
    (projects.into_iter())
        .flat_map(|(_, questions)| questions)
        .map(|(_, question)| question)
        .collect()
}

/// The folder above the `questions/` folder that holds `path`, or the
/// question's own folder when no `questions/` folder holds it. The root is
/// `/`.
fn project_of(path: &str) -> String {
    let folder = match path.rsplit_once("/questions/") {
        Some((project, id)) if !id.contains('/') => project,
        _ => path.rsplit_once('/').map_or("", |(folder, _)| folder),
    };
    if folder.is_empty() { "/" } else { folder }.to_owned()
}

/// `None` when `body` lacks the shape `open` returns, as a mounted board's may.
fn readable(path: &str, body: &Value, critical: bool) -> Option<(Instant, Waiting)> {
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
        session: asked.by.session?,
        options,
        recommendation: asked.recommendation,
        why: asked.why,
        blocks: asked.blocks,
        blocked_work: asked.blocked_work,
        note: asked.note,
        mount: asked.mount,
        context_path: asked.context_path,
        project: project_of(path),
        critical,
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
pub(crate) mod tests;

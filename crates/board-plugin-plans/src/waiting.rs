//! `plans.waiting`: how many questions wait for the owner, and one line that
//! says so, which an agent can end its reply with. A question that blocks a
//! change `changes.ready` lists is counted there, so it is left out here.

use std::collections::BTreeSet;

use board_ops::{Host, PluginError};
use board_plugin::Target;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The questions below `path` to count.
#[derive(Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "WaitingInput")]
pub(crate) struct Input {
    /// Only questions below this logical path, such as /projects/board;
    /// every question when left out.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            path: "/".to_owned(),
        }
    }
}

/// How many questions wait, as `plans.open` counts them, less those that
/// block a change ready to land. The board-plans mod's band draws `total`
/// and `blocking`.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WaitingOutput")]
pub(crate) struct Output {
    /// The readable open questions that no answer names, `plans.open`'s total.
    total: usize,
    /// How many of them block a change or task that is still going, the
    /// ones `plans.open` marks critical.
    blocking: usize,
    /// The open questions Board marks degraded or this plugin cannot read.
    unreadable: usize,
    /// The open questions left out because they block a change that
    /// `changes.ready` lists; 0 when that call is refused.
    ready: usize,
    /// One sentence that says how many questions wait and what to run.
    line: String,
}

pub(crate) fn waiting(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("plans", "waiting", input)?;
    let (mut found, unreadable) = crate::open::waiting(host, &path)?;
    let asked: Vec<&str> = (found.iter())
        .filter(|question| question.blocks.is_some())
        .map(|question| question.path.as_str())
        .collect();
    let ready = ready_questions(host, &asked);
    let before = found.len();
    found.retain(|question| !ready.contains(&question.path));
    let ready = before - found.len();
    let total = found.len();
    let blocking = found.iter().filter(|question| question.critical).count();
    let output = Output {
        total,
        blocking,
        unreadable,
        ready,
        line: line(total, blocking, unreadable),
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings and numbers"))
}

/// The questions of `asked` that block a change `changes.ready` lists,
/// none when `asked` is empty or Board refuses the call, such as in a board
/// with no changes plugin.
fn ready_questions(host: &dyn Host, asked: &[&str]) -> BTreeSet<String> {
    if asked.is_empty() {
        return BTreeSet::new();
    }
    let target: Target = "changes.ready"
        .parse()
        .expect("changes.ready is <plugin>.<capability>");
    let input = json!({"failing": false, "questions": asked, "limit": 10_000});
    let Ok(report) = host.call(&target, &input) else {
        return BTreeSet::new();
    };
    (report
        .output
        .get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|change| change.get("questions")?.as_array())
    .flatten()
    .filter_map(Value::as_str)
    .map(str::to_owned)
    .collect()
}

fn line(count: usize, blocking: usize, unreadable: usize) -> String {
    let (noun, verb, pronoun) = words(count);
    let blocks = match blocking {
        0 => String::new(),
        _ if count == 1 => " and blocks work".to_owned(),
        some => format!(", {some} of them blocking work"),
    };
    let apart = match unreadable {
        0 => String::new(),
        more => format!(", and {more} more cannot be read"),
    };
    match count {
        0 if unreadable == 0 => "No questions are waiting for you.".to_owned(),
        0 => {
            let (noun, verb, pronoun) = words(unreadable);
            format!(
                "{unreadable} {noun} {verb} waiting for you but cannot be read. Run `board check` on {pronoun}."
            )
        }
        _ => format!(
            "{count} {noun} {verb} waiting for you{blocks}{apart}. Run /ask to answer {pronoun}."
        ),
    }
}

/// The noun, verb and pronoun for `count` questions.
fn words(count: usize) -> (&'static str, &'static str, &'static str) {
    if count == 1 {
        ("question", "is", "it")
    } else {
        ("questions", "are", "them")
    }
}

#[cfg(test)]
mod tests;

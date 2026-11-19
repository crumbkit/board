//! `plans.waiting`: how many questions wait for the owner, and one line that
//! says so, which an agent can end its reply with.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// How many questions wait, as `plans.open` counts them. The board-plans
/// mod's band draws `total` and `blocking`.
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
    /// One sentence that says how many questions wait and what to run.
    line: String,
}

pub(crate) fn waiting(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("plans", "waiting", input)?;
    let (found, unreadable) = crate::open::waiting(host, &path)?;
    let total = found.len();
    let blocking = found.iter().filter(|question| question.critical).count();
    let output = Output {
        total,
        blocking,
        unreadable,
        line: line(total, blocking, unreadable),
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings and numbers"))
}

fn line(count: usize, blocking: usize, unreadable: usize) -> String {
    let (noun, verb, pronoun) = if count == 1 {
        ("question", "is", "it")
    } else {
        ("questions", "are", "them")
    };
    let blocks = match blocking {
        0 => String::new(),
        _ if count == 1 => ", and it blocks work".to_owned(),
        some => format!(", {some} of them blocking work"),
    };
    let apart = match unreadable {
        0 => String::new(),
        more => format!(", and {more} more cannot be read"),
    };
    match count {
        0 if unreadable == 0 => "No questions are waiting for you.".to_owned(),
        0 => {
            format!("No readable questions are waiting for you{apart}. Run `board check` on them.")
        }
        _ => format!(
            "{count} {noun} {verb} waiting for you{blocks}{apart}. Run /ask to answer {pronoun}."
        ),
    }
}

#[cfg(test)]
mod tests;

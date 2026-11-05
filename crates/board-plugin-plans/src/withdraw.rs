//! `plans.withdraw`: marks an open question withdrawn, with the reason, so
//! that `plans.open` stops listing it.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::question::{self, answer_path_of};

/// The question at `path` to withdraw, and why.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WithdrawInput")]
pub(crate) struct Input {
    /// The question's logical path, such as /projects/board/questions/20261024T1830Z-k3.
    #[schemars(pattern("^/"), extend("format" = "board-path"))]
    path: String,
    /// Why the question no longer matters, which the question keeps as `withdrawn_reason`.
    #[schemars(length(min = 1))]
    reason: String,
}

/// The question as withdrawn.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WithdrawOutput")]
pub(crate) struct Output {
    /// The question's path, whose status is now withdrawn.
    question: String,
    /// The reason recorded.
    reason: String,
}

pub(crate) fn withdraw(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, reason } = PluginError::read_input("plans", "withdraw", input)?;
    answer_path_of("withdraw", &path)?;
    let mut asked = question::read(host, "withdraw", &path)?;
    asked.still_open("withdraw", &path)?;
    asked.fields.insert("status".to_owned(), json!("withdrawn"));
    asked
        .fields
        .insert("withdrawn_reason".to_owned(), json!(reason));
    let lines = [json!({"path": path, "expect": asked.blob, "record": asked.fields})];
    (host.put(&lines))
        .map_err(|error| PluginError::board_refused("plans", "put --batch", &error.refusal()))?;
    let output = Output {
        question: path,
        reason,
    };
    Ok(serde_json::to_value(output).expect("Output holds only strings"))
}

#[cfg(test)]
mod tests;

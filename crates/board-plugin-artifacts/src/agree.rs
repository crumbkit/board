//! `artifacts.agree`: move an artifact to `agreed`, naming the owner's
//! decision that accepts it.

use board_ops::{Host, PluginError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::held::{self, refusal};
use crate::records::{Answer, Decision, State};

/// The artifact and the owner's decision that accepts it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgreeInput")]
pub(crate) struct Input {
    /// The artifact's record, such as /artifacts/AbCdEf0123456789xyzQ.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// The owner's plans/decision/v1 that accepts the artifact. It names
    /// the owner's plans/answer/v1 in its refs.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    decision: String,
}

/// The artifact, now agreed, and the decision that agreed it.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgreeOutput")]
pub(crate) struct Output {
    path: String,
    state: State,
    agreed_by: String,
}

/// Sets the artifact's state to `agreed` and `agreed_by` to the decision,
/// once the decision is the owner's.
pub(crate) fn agree(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, decision } = PluginError::read_input("artifacts", "agree", input)?;
    let (mut record, blob) = held::artifact(host, &path)?;
    if record.state == State::Superseded {
        return Err(refusal(
            "artifacts::state::settled",
            format!(
                "{path} is superseded, so it cannot be agreed; agree the artifact that replaced it, \
                 {}",
                record
                    .superseded_by
                    .as_deref()
                    .unwrap_or("which its record does not name")
            ),
            &[("path", &path), ("state", "superseded")],
        ));
    }
    owners(host, &decision)?;
    record.state = State::Agreed;
    record.agreed_by = Some(decision.clone());
    held::put(
        host,
        &[json!({"path": path, "expect": blob, "record": record})],
    )?;
    let output = Output {
        path,
        state: State::Agreed,
        agreed_by: decision,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings"))
}

/// Refuses with `artifacts::agree::no_decision` unless `decision` is a
/// `plans/decision/v1` that a person or agent wrote, not a hook's stub,
/// and that names one of the owner's answers in its `refs`. An answer holds
/// the owner's own words, so it is what makes the decision the owner's.
fn owners(host: &dyn Host, decision: &str) -> Result<(), PluginError> {
    let no_decision = |why: &str| {
        refusal(
            "artifacts::agree::no_decision",
            format!(
                "{decision} {why}; record the owner's answer with the plans plugin, then a \
                 plans/decision/v1 whose refs name that answer, and pass the decision's path"
            ),
            &[("decision", decision)],
        )
    };
    let Some((record, _)) = held::get(host, decision)? else {
        return Err(no_decision("holds no record"));
    };
    if record["$type"] != Decision::TYPE {
        return Err(no_decision("is not a plans/decision/v1 record"));
    }
    if record.get("written_by").is_some() {
        return Err(no_decision(
            "is a stub a hook wrote, not the owner's decision",
        ));
    }
    let refs = (record["refs"].as_array().into_iter().flatten()).filter_map(Value::as_str);
    for answer in refs {
        if let Ok(Some((held, _))) = held::get(host, answer)
            && held["$type"] == Answer::TYPE
        {
            return Ok(());
        }
    }
    Err(no_decision(
        "names none of the owner's plans/answer/v1 records in its refs",
    ))
}

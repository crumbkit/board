//! `artifacts.agree`: move an artifact to `agreed` once the owner approves
//! it with Touch ID, naming the decision that accepts it.

use board_ops::{Host, PluginError, Shown};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::held::{self, refusal, refused};
use crate::records::{ArtifactRecord, Decision, State};

/// The capability Board asks the owner to approve.
const AGREE: &str = "artifacts.agree";
/// Board has written the pending effect and the owner has not answered.
const ASKED: &str = "board::settings::asked";
/// The owner refused the effect.
const REFUSED: &str = "board::settings::refused";

/// The artifact and the decision that accepts it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AgreeInput")]
pub(crate) struct Input {
    /// The artifact's record, such as /artifacts/AbCdEf0123456789xyzQ.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// The plans/decision/v1 that accepts the artifact. The owner approves
    /// the artifact with this decision named in it.
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

/// Sets the artifact's state to `agreed` and `agreed_by` to the decision
/// once the owner approves that exact record with Touch ID. Until then it
/// refuses with `artifacts::agree::asked` and writes nothing.
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
    is_decision(host, &decision)?;
    record.state = State::Agreed;
    record.agreed_by = Some(decision.clone());
    approved(host, &path, &record)?;
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

/// Refuses with `artifacts::agree::no_decision` unless `decision` holds a
/// `plans/decision/v1` record.
fn is_decision(host: &dyn Host, decision: &str) -> Result<(), PluginError> {
    let why = match held::get(host, decision)? {
        None => "holds no record",
        Some((record, _)) if record["$type"] != Decision::TYPE => {
            "is not a plans/decision/v1 record"
        }
        Some(_) => return Ok(()),
    };
    Err(refusal(
        "artifacts::agree::no_decision",
        format!(
            "{decision} {why}; pass the path of the plans/decision/v1 that accepts the artifact"
        ),
        &[("decision", decision)],
    ))
}

/// Asks the owner, through Board, to approve `record` as the agreed record
/// at `path`. The approval covers that exact record, so it admits only an
/// agree of the same artifact with the same decision. Only the owner's
/// Touch ID signs an approval, so a record an agent writes cannot stand in
/// for it.
fn approved(host: &dyn Host, path: &str, record: &ArtifactRecord) -> Result<(), PluginError> {
    let shown = serde_json::to_vec(record).expect("an artifact record serializes");
    let at = held::logical(path)?;
    let Err(refusal) = host.decide(AGREE, &at, Some(&Shown::Record(shown))) else {
        return Ok(());
    };
    let effect = (refusal.fields.get("effect").and_then(Value::as_str)).unwrap_or_default();
    match refusal.code.as_str() {
        ASKED => {
            let mut asked = held::refusal(
                "artifacts::agree::asked",
                format!("{path} waits for the owner to approve agreeing it ({effect})"),
                &[("path", path), ("effect", effect)],
            );
            asked.hint = Some(format!(
                "the owner approves with `board answer {effect} approve`, which asks for Touch ID; \
                 then run the same agree again"
            ));
            Err(asked)
        }
        REFUSED => Err(held::refusal(
            "artifacts::agree::refused",
            format!(
                "the owner refused agreeing {path} ({effect}); the owner can ask again by \
                 deleting /effects/seen/{effect} in the personal board"
            ),
            &[("path", path), ("effect", effect)],
        )),
        _ => Err(refused("decide", &refusal)),
    }
}

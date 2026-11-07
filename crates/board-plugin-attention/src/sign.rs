//! `sign`: show the owner an order through Board's signer and, once they
//! sign it with Touch ID, write their signature into the order.

use board_ops::{Host, PluginError, SignOrderError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{order, refusal, refused, verify};

/// The order to sign.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SignInput")]
pub(crate) struct Input {
    /// The order's logical path, such as /orders/no-force-push.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    pub(crate) path: String,
}

/// The order the owner signed.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "SignOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) path: String,
}

/// Signs the order at the input's path. An order whose day has passed is
/// refused before the owner sees it. The put expects the blob the order
/// was read from, so an order edited while the owner read it is refused
/// with Board's `board::put::stale` and keeps its new text unsigned.
pub(crate) fn sign(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("attention", "sign", input)?;
    let mut held = order::read(host, &path)?;
    verify::in_force(
        &path,
        held.expires,
        host.settings().now,
        "set a later expires in the draft before asking the owner to sign it",
    )?;
    let signature = (host.sign_order(&held.order)).map_err(|error| unsigned(&path, &error))?;
    held.record["signature"] = json!(signature);
    let line = json!({"path": path, "expect": held.blob, "record": held.record});
    (host.put(&[line])).map_err(|error| refused("put --batch", &error.refusal()))?;
    Ok(serde_json::to_value(Output { path }).expect("the output is a plain string"))
}

/// The refusal of an order the owner did not sign, by why.
pub(crate) fn unsigned(path: &str, error: &SignOrderError) -> PluginError {
    match error {
        SignOrderError::Changes { feedback } => refusal(
            "attention::sign::declined",
            format!(
                "the owner did not sign {path} and asked for these changes:\n{feedback}\nrevise \
                 the draft, then ask the owner to sign it again"
            ),
            [("path", json!(path)), ("feedback", json!(feedback))],
        ),
        SignOrderError::Cancelled => refusal(
            "attention::sign::cancelled",
            format!("{error}; {path} is still waiting for the owner's signature"),
            [("path", json!(path))],
        ),
        SignOrderError::NoSigner | SignOrderError::NotOffered => refusal(
            "attention::sign::no_signer",
            format!("{path} was not signed: {error}; sign it with the release build of board"),
            [("path", json!(path))],
        ),
        SignOrderError::NoCache
        | SignOrderError::Unwritten { .. }
        | SignOrderError::Failed { .. }
        | SignOrderError::Unverified { .. } => refusal(
            "attention::sign::failed",
            format!("{path} was not signed: {error}"),
            [("path", json!(path))],
        ),
    }
}

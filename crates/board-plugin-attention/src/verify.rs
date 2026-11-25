//! `verify`: an order and its context, when the owner's key signed it and
//! it has not expired.

use std::time::{Duration, SystemTime};

use board_ops::{Host, PluginError, VerifyOrderError};
use board_plugin::shape::Date as DateShape;
use board_repos::{Date, today};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::order;
use crate::refusal;

/// The order at a path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "VerifyInput")]
pub(crate) struct Input {
    /// The order's logical path, such as /orders/no-force-push.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    pub(crate) path: String,
}

/// An order whose signature verifies against an allowed signer, and has
/// not expired.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "VerifyOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) path: String,
    /// The principal in the allowed signers file whose key signed it.
    pub(crate) signer: String,
    /// The binding instruction.
    pub(crate) order: String,
    #[schemars(with = "DateShape")]
    pub(crate) expires: String,
    /// The context file's path.
    pub(crate) context: String,
    /// The non-binding context, as the owner read it when signing.
    pub(crate) context_text: String,
}

pub(crate) fn verify(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("attention", "verify", input)?;
    let output = verified(host, &path)?;
    Ok(serde_json::to_value(output).expect("the output is plain strings"))
}

/// The order at `path`, when its signature verifies and it has not
/// expired.
pub(crate) fn verified(host: &dyn Host, path: &str) -> Result<Output, PluginError> {
    let held = order::read(host, path)?;
    let Some(signature) = &held.fields.signature else {
        return Err(refusal(
            "attention::order::unsigned",
            format!(
                "{path} has no signature, so the owner has not approved it; ask the owner to sign it"
            ),
            [("path", json!(path))],
        ));
    };
    let signer =
        (host.verify_order(&held.order, signature)).map_err(|error| unverified(path, &error))?;
    in_force(
        path,
        held.expires,
        host.settings().now,
        "ask the owner for a new order",
    )?;
    Ok(Output {
        path: path.to_owned(),
        signer: signer.to_owned(),
        order: held.fields.order,
        expires: held.expires.to_string(),
        context: held.fields.context,
        context_text: held.order.context_text,
    })
}

/// Refuses an order whose day has passed at `now` with
/// `attention::order::expired`, whose message ends with `next`.
pub(crate) fn in_force(
    path: &str,
    expires: Date,
    now: SystemTime,
    next: &str,
) -> Result<(), PluginError> {
    if !expired(expires, now) {
        return Ok(());
    }
    let expires = expires.to_string();
    Err(refusal(
        "attention::order::expired",
        format!("{path} expired on {expires}; {next}"),
        [("path", json!(path)), ("expires", json!(expires))],
    ))
}

/// Whether an order that binds through `expires` has expired at `now`: its
/// day has passed in UTC, as the Python plugin counted it.
pub(crate) fn expired(expires: Date, now: SystemTime) -> bool {
    expires < today(now, 0, Duration::ZERO)
}

/// The refusal of an order whose signature did not verify, by why.
pub(crate) fn unverified(path: &str, error: &VerifyOrderError) -> PluginError {
    match error {
        VerifyOrderError::NoneNamed | VerifyOrderError::NoFile { .. } => refusal(
            "attention::signers::missing",
            format!("{error}; the owner adds their key to it, as the plugin's README says"),
            [("path", json!(path))],
        ),
        VerifyOrderError::Untrusted { file, .. } => refusal(
            "attention::signers::untrusted",
            format!(
                "{error}; make the owner the file's owner and let no one else write it, then \
                 verify again"
            ),
            [("path", json!(path)), ("file", json!(file))],
        ),
        VerifyOrderError::Bad { file, .. } => refusal(
            "attention::order::bad_signature",
            format!(
                "{path}'s signature does not verify against {}: the order, its expiry or its \
                 context changed after signing, or another key signed it",
                file.display()
            ),
            [("path", json!(path))],
        ),
        VerifyOrderError::Failed { .. } | VerifyOrderError::NotOffered => refusal(
            "attention::verify::failed",
            format!("{path}'s signature was not checked: {error}"),
            [("path", json!(path))],
        ),
    }
}

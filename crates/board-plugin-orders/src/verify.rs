//! `verify`: an order and its context, when the owner's key signed it and
//! it has not expired.

use std::time::{Duration, SystemTime};

use board_model::{Act, Need, Order};
use board_ops::{Host, PluginError, VerifyOrderError};
use board_plugin::PRINCIPAL;
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
    /// The principal in the allowed signers file whose key signed it, or
    /// `owner` for an order the owner approved in chat.
    pub(crate) signer: String,
    /// How the owner approved it: `signed by owner`, or `approved in chat,
    /// not signed` for an order an answer approved where signing takes an
    /// answer for orders.
    pub(crate) approval: String,
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
    let Input { path } = PluginError::read_input("orders", "verify", input)?;
    let output = verified(host, &path)?;
    Ok(serde_json::to_value(output).expect("the output is plain strings"))
}

/// How a signed order reads.
pub(crate) const SIGNED: &str = "signed by owner";

/// How an order the owner approved in chat reads.
pub(crate) const APPROVED: &str = "approved in chat, not signed";

/// The order at `path`, when its signature verifies, or the answer it names
/// approves it where signing takes an answer for orders, and it has not
/// expired.
pub(crate) fn verified(host: &dyn Host, path: &str) -> Result<Output, PluginError> {
    let held = order::read(host, path)?;
    let approval = match (&held.fields.signature, &held.fields.approved) {
        (Some(signature), _) => {
            let signer = (host.verify_order(&held.order, signature))
                .map_err(|error| unverified(path, &error))?;
            by_owner(path, signer)?;
            SIGNED
        }
        (None, Some(approved)) => {
            in_chat(host, path, &held.order, approved)?;
            APPROVED
        }
        (None, None) => {
            return Err(refusal(
                "orders::order::unsigned",
                format!(
                    "{path} has no signature, so the owner has not approved it; ask the owner to \
                     sign it, or, where signing takes an answer for orders, to approve it in chat \
                     through a question that names {}",
                    held.order.digest()
                ),
                [
                    ("path", json!(path)),
                    ("digest", json!(held.order.digest())),
                ],
            ));
        }
    };
    in_force(
        path,
        held.expires,
        host.settings().now,
        "ask the owner for a new order",
    )?;
    Ok(Output {
        path: path.to_owned(),
        signer: PRINCIPAL.to_owned(),
        approval: approval.to_owned(),
        order: held.fields.order,
        expires: held.expires.to_string(),
        context: held.fields.context,
        context_text: held.order.context_text,
    })
}

/// Refuses a signature of the order at `path` that verified under
/// `signer`, unless `signer` is the owner, so an order another principal
/// signed is never reported as signed by the owner.
pub(crate) fn by_owner(path: &str, signer: &str) -> Result<(), PluginError> {
    if signer == PRINCIPAL {
        return Ok(());
    }
    Err(refusal(
        "orders::order::bad_signature",
        format!(
            "{path}'s signature verifies as {signer}'s, not {PRINCIPAL}'s, so the owner did not \
             sign it; ask the owner to sign it"
        ),
        [("path", json!(path)), ("signer", json!(signer))],
    ))
}

/// Refuses unless the answer at `approved` approves `order` in chat: signing
/// takes an answer for orders, and the answer is the owner's choice of
/// `approve` in a question, asked before it, that names the order's digest.
pub(crate) fn in_chat(
    host: &dyn Host,
    path: &str,
    order: &Order,
    approved: &str,
) -> Result<(), PluginError> {
    let fields = [("path", json!(path)), ("approved", json!(approved))];
    match host.signing().need(Act::Order) {
        Need::Answer => {}
        Need::Signature => {
            return Err(refusal(
                "orders::order::unsigned",
                format!(
                    "{path} was approved in chat, but signing is on and orders need Touch ID; ask \
                     the owner to sign it"
                ),
                fields,
            ));
        }
        Need::FileMissing => {
            return Err(refusal(
                "orders::signers::missing",
                format!(
                    "{path} was approved in chat, but Board has verified a signature before and \
                     {} is missing, so orders need it restored",
                    host.signers_file()
                ),
                fields,
            ));
        }
    }
    let answer = order::record(host, approved)?;
    let question = (answer.get("question").and_then(Value::as_str))
        .map(|question| order::record(host, question))
        .transpose()?;
    board_model::approving(&answer, question.as_ref(), &order.digest()).map_err(|why| {
        refusal(
            "orders::order::not_approved",
            format!("{path} names {approved}, which does not approve it: {why}"),
            fields,
        )
    })?;
    Ok(())
}

/// Refuses an order whose day has passed at `now` with
/// `orders::order::expired`, whose message ends with `next`.
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
        "orders::order::expired",
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
            "orders::signers::missing",
            format!("{error}; the owner adds their key to it, as the plugin's README says"),
            [("path", json!(path))],
        ),
        VerifyOrderError::Untrusted { file, .. } => refusal(
            "orders::signers::untrusted",
            format!(
                "{error}; make the owner the file's owner and let no one else write it, then \
                 verify again"
            ),
            [("path", json!(path)), ("file", json!(file))],
        ),
        VerifyOrderError::Bad { file, .. } => refusal(
            "orders::order::bad_signature",
            format!(
                "{path}'s signature does not verify against {}: the order, its expiry or its \
                 context changed after signing, or another key signed it",
                file.display()
            ),
            [("path", json!(path))],
        ),
        VerifyOrderError::Failed { .. } | VerifyOrderError::NotOffered => refusal(
            "orders::verify::failed",
            format!("{path}'s signature was not checked: {error}"),
            [("path", json!(path))],
        ),
    }
}

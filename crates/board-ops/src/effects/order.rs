//! Signing and verifying an order, which a built-in plugin asks
//! for through `Host::sign_order` and `Host::verify_order`
//! (docs/architecture/writes/signing.md).

use std::path::{Path, PathBuf};

use board_model::{ORDER_TYPE, Order, PluginCode, PluginName, Refusal};
use board_plugin::{PRINCIPAL, SignError, Signers, Verb};
use serde_json::{Map, json};

use super::Signing;

/// Why an order has no signature.
#[derive(Debug, thiserror::Error)]
pub enum SignOrderError {
    #[error("the owner did not sign the order and asked for changes: {feedback}")]
    Changes { feedback: String },
    #[error("the owner cancelled, so the order was not signed")]
    Cancelled,
    #[error("no signer is found: the owner's home is not known, or board is a debug build")]
    NoSigner,
    #[error("no cache directory is named for the order's statement")]
    NoCache,
    #[error("the order's statement cannot be written: {source}")]
    Unwritten { source: std::io::Error },
    #[error("the signer did not sign the order: {source}")]
    Failed { source: SignError },
    #[error("the signature the signer made of the order does not verify: {source}")]
    Unverified { source: VerifyOrderError },
    #[error("this Board offers a built-in plugin no signer")]
    NotOffered,
}

/// Why an order's signature does not verify.
#[derive(Debug, thiserror::Error)]
pub enum VerifyOrderError {
    #[error("no allowed signers file for orders is named: the owner's home is not known")]
    NoneNamed,
    #[error("no allowed signers file for orders is at {}", file.display())]
    NoFile { file: PathBuf },
    #[error("{} is not trusted: {why}", file.display())]
    Untrusted { file: PathBuf, why: &'static str },
    #[error("the order's signature does not verify for {PRINCIPAL} against {}: {stderr}", file.display())]
    Bad { file: PathBuf, stderr: String },
    #[error("the verifier did not check the order's signature: {source}")]
    Failed { source: SignError },
    #[error("this Board offers a built-in plugin no verifier")]
    NotOffered,
}

/// The owner's signature of `order`'s statement, which the signer shows
/// them from a 0600 file in the cache under `root`'s name, removed once it
/// returns. It is checked as `verify_order` checks a stored order before it
/// is returned.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module builds the order statement it signs, is the one caller of the signer, and checks the signature with verify_order"
)]
pub(crate) fn sign_order(
    signing: &Signing,
    root: &Path,
    order: &Order,
) -> Result<String, SignOrderError> {
    let signer = (signing.signer.as_ref()).ok_or(SignOrderError::NoSigner)?;
    let cache = (signing.cache.as_ref()).ok_or(SignOrderError::NoCache)?;
    let statement = order.statement();
    let file = board_index::write_statement(cache, root, statement.as_bytes())
        .map_err(|source| SignOrderError::Unwritten { source })?;
    let signature =
        board_plugin::sign(signer, &file, &signing.environment, Verb::Sign).map_err(|error| {
            match error {
                SignError::Declined { feedback } => SignOrderError::Changes { feedback },
                SignError::Cancelled { .. } => SignOrderError::Cancelled,
                source => SignOrderError::Failed { source },
            }
        })?;
    drop(file);
    verify_order(signing, order, &signature)
        .map_err(|source| SignOrderError::Unverified { source })?;
    Ok(signature)
}

/// Whose key made an order's signature, and the allowed signers file that
/// admitted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderSigner {
    /// The principal, `owner`.
    pub principal: &'static str,
    /// The allowed signers file whose line admitted the key.
    pub file: PathBuf,
    /// Whether only the attention allowed signers file admitted it, which
    /// Board reads for one more release.
    pub legacy: bool,
}

/// The principal, `owner`, whose key made `signature` of `order`'s
/// statement in the order's namespace, and the file that admitted it:
/// Board's own allowed signers file, or else the attention file. A Board
/// file Board does not trust refuses the order, as it refuses an approval;
/// a key neither file admits is refused against Board's file, or against
/// the attention file when Board's names none or is missing.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module builds the order statement it verifies and is the one caller of the verifier"
)]
pub(crate) fn verify_order(
    signing: &Signing,
    order: &Order,
    signature: &str,
) -> Result<OrderSigner, VerifyOrderError> {
    let statement = order.statement();
    let against = |file: Option<&Path>, legacy: bool| {
        let file = file.ok_or(VerifyOrderError::NoneNamed)?;
        board_plugin::verify(file, &statement, signature).map_err(|error| match error {
            SignError::NoSigners { file } => VerifyOrderError::NoFile { file },
            SignError::Untrusted { file, why } => VerifyOrderError::Untrusted { file, why },
            SignError::Bad { file, stderr } => VerifyOrderError::Bad { file, stderr },
            source => VerifyOrderError::Failed { source },
        })?;
        Ok(OrderSigner {
            principal: PRINCIPAL,
            file: file.to_owned(),
            legacy,
        })
    };
    let board = match against(signing.allowed.as_deref(), false) {
        Ok(signer) => return Ok(signer),
        Err(error @ (VerifyOrderError::Untrusted { .. } | VerifyOrderError::Failed { .. })) => {
            return Err(error);
        }
        Err(error) => error,
    };
    against(signing.attention_allowed.as_deref(), true).map_err(|attention| {
        match (board, attention) {
            (
                board @ (VerifyOrderError::NoneNamed | VerifyOrderError::NoFile { .. }),
                VerifyOrderError::NoneNamed,
            ) => board,
            (VerifyOrderError::NoneNamed | VerifyOrderError::NoFile { .. }, attention) => attention,
            (board, _) => board,
        }
    })
}

/// `orders::signers::legacy`, the orders plugin's warning that only the
/// attention file admitted `signer`'s key, or none when Board's own file did.
pub(crate) fn legacy(signing: &Signing, signer: &OrderSigner) -> Option<Refusal> {
    if !signer.legacy {
        return None;
    }
    let orders: PluginName = "orders".parse().expect("orders is a plugin name");
    let code = PluginCode::parse(&orders, "orders::signers::legacy".to_owned())
        .expect("orders::signers::legacy is a code of the orders plugin");
    let board = (signing.allowed.as_deref()).map_or_else(
        || "Board's allowed signers file".to_owned(),
        |file| file.display().to_string(),
    );
    let file = signer.file.display().to_string();
    let message = format!(
        "an order's key is admitted only by {file}, which Board stops reading in its next release"
    );
    let fields = Map::from_iter([("file".to_owned(), json!(file))]);
    let hint =
        format!("run `board --personal orders signers`, read its lines and write them to {board}");
    Some(Refusal::plugin(code, message, fields).with_hint(hint))
}

/// Board's allowed signers file with the attention file's keys merged in
/// for orders, as `board orders signers` prints it. It reads both files and
/// writes neither.
pub(crate) fn order_signers(signing: &Signing) -> Result<Signers, VerifyOrderError> {
    let board = (signing.allowed.as_deref()).ok_or(VerifyOrderError::NoneNamed)?;
    let attention = signing.attention_allowed.as_deref();
    board_plugin::signers(board, attention, ORDER_TYPE).map_err(|error| match error {
        SignError::Untrusted { file, why } => VerifyOrderError::Untrusted { file, why },
        source => VerifyOrderError::Failed { source },
    })
}

#[cfg(test)]
mod tests;

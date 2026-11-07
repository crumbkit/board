//! Signing and verifying an attention order, which a built-in plugin asks
//! for through `Host::sign_order` and `Host::verify_order`
//! (docs/architecture/writes/signing.md).

use std::path::{Path, PathBuf};

use board_model::Order;
use board_plugin::{PRINCIPAL, SignError, Verb};

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
/// returns. It is checked against the attention allowed signers file before
/// it is returned.
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

/// The principal, `owner`, whose key in the attention allowed signers file
/// made `signature` of `order`'s statement in the order's namespace.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module builds the order statement it verifies and is the one caller of the verifier"
)]
pub(crate) fn verify_order(
    signing: &Signing,
    order: &Order,
    signature: &str,
) -> Result<&'static str, VerifyOrderError> {
    let allowed = (signing.attention_allowed.as_deref()).ok_or(VerifyOrderError::NoneNamed)?;
    board_plugin::verify(allowed, &order.statement(), signature).map_err(|error| match error {
        SignError::NoSigners { file } => VerifyOrderError::NoFile { file },
        SignError::Untrusted { file, why } => VerifyOrderError::Untrusted { file, why },
        SignError::Bad { file, stderr } => VerifyOrderError::Bad { file, stderr },
        source => VerifyOrderError::Failed { source },
    })?;
    Ok(PRINCIPAL)
}

#[cfg(test)]
mod tests;

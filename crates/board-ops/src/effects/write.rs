//! Signing and verifying the owner's write of a record that a built-in
//! plugin keeps and trusts only with the owner's signature, which it asks
//! for through `Host::sign_owner_write` and `Host::verify_owner_write`
//! (docs/architecture/writes/signing.md).

use std::path::Path;

use board_model::Effect;
use board_plugin::{SignError, Verb};
use serde_json::Value;

use super::Signing;

/// The board an owner's write a plugin keeps names: the personal board,
/// wherever it is cloned, so a signature survives a move of the clone.
const PERSONAL: &str = "personal";

/// Why the owner's write of a record has no good signature.
#[derive(Debug, thiserror::Error)]
pub enum OwnerWriteError {
    #[error("the owner did not sign the write and asked for changes: {feedback}")]
    Changes { feedback: String },
    #[error("the owner cancelled, so the write was not signed")]
    Cancelled,
    #[error("no signer is found: the owner's home is not known, or board is a debug build")]
    NoSigner,
    #[error("no cache directory is named for the write's statement")]
    NoCache,
    #[error("no allowed signers file is named: the owner's home is not known")]
    NoneNamed,
    #[error("the write's statement cannot be written: {source}")]
    Unwritten { source: std::io::Error },
    #[error("the signer did not sign the write: {source}")]
    Failed { source: SignError },
    #[error("the write's signature does not verify: {source}")]
    Bad { source: SignError },
    #[error("this Board offers a built-in plugin no signer or verifier")]
    NotOffered,
}

/// The statement of the owner's put of `record` at `path` in the personal
/// board.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module builds the owner-write statement it signs and verifies"
)]
fn statement(path: &str, record: &Value) -> String {
    Effect::owner_write(PERSONAL, board_model::PUT_OP, path, Some(record), None)
}

/// The owner's signature of their put of `record` at `path` in the
/// personal board, which the signer shows them from a 0600 file in the
/// cache under `root`'s name. It is checked against the allowed signers
/// file before it is returned.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module is the one caller of the signer"
)]
pub(crate) fn sign_owner_write(
    signing: &Signing,
    root: &Path,
    path: &str,
    record: &Value,
) -> Result<String, OwnerWriteError> {
    let signer = (signing.signer.as_ref()).ok_or(OwnerWriteError::NoSigner)?;
    let cache = (signing.cache.as_ref()).ok_or(OwnerWriteError::NoCache)?;
    let statement = statement(path, record);
    let file = board_index::write_statement(cache, root, statement.as_bytes())
        .map_err(|source| OwnerWriteError::Unwritten { source })?;
    let signature = board_plugin::sign(signer, &file, &signing.environment, Verb::Approve)
        .map_err(|error| match error {
            SignError::Declined { feedback } => OwnerWriteError::Changes { feedback },
            SignError::Cancelled { .. } => OwnerWriteError::Cancelled,
            source => OwnerWriteError::Failed { source },
        })?;
    drop(file);
    verify_owner_write(signing, path, record, &signature)?;
    Ok(signature)
}

/// Checks that `signature` is the owner's signature of their put of
/// `record` at `path` in the personal board, by a key the allowed signers
/// file lists.
#[expect(
    clippy::disallowed_methods,
    reason = "board-ops's effects module is the one caller of the verifier"
)]
pub(crate) fn verify_owner_write(
    signing: &Signing,
    path: &str,
    record: &Value,
    signature: &str,
) -> Result<(), OwnerWriteError> {
    let allowed = (signing.allowed.as_deref()).ok_or(OwnerWriteError::NoneNamed)?;
    board_plugin::verify(allowed, &statement(path, record), signature)
        .map_err(|source| OwnerWriteError::Bad { source })
}

#[cfg(test)]
mod tests;

//! `add`: builds a board's record, has the owner sign it with Touch ID
//! through Board's signer, and writes it with the signature. Without a
//! signature nothing is written.

use board_ops::{Host, OwnerWriteError, PluginError};
use board_plugin::shape::{Record, Slug};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::records::{BOARDS, BoardRecord};
use crate::{PLUGIN, refusal};

/// The board to add.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AddInput")]
pub(crate) struct Input {
    /// The board's name, the last segment of its record's path.
    #[schemars(with = "Slug")]
    pub(crate) name: String,
    /// The board's directory: absolute, or under the home as `~/...`.
    #[schemars(regex(pattern = r"^(/|~/).+"))]
    pub(crate) path: String,
    /// Each URL the board may push to.
    #[schemars(length(min = 1))]
    pub(crate) origin: Vec<String>,
    /// The branch to keep in step, `main` when left out.
    #[schemars(regex(pattern = r"^[A-Za-z0-9._/-]+$"))]
    pub(crate) branch: Option<String>,
}

/// The record written.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "AddOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) path: String,
}

pub(crate) fn add(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "add", input)?;
    let path = format!("{BOARDS}{}", given.name);
    let mut record = json!({"$type": BoardRecord::TYPE, "path": given.path, "origins": given.origin});
    if let Some(branch) = given.branch {
        record["branch"] = json!(branch);
    }
    let signature = (host.sign_owner_write(&path, &record)).map_err(|error| unsigned(&error))?;
    record["signature"] = json!(signature);
    let line = json!({"path": path, "record": record});
    (host.put(&[line]))
        .map_err(|error| PluginError::board_refused(PLUGIN, "put --batch", &error.refusal()))?;
    Ok(json!(Output { path }))
}

/// The refusal of a record the owner did not sign, by why.
fn unsigned(error: &OwnerWriteError) -> PluginError {
    let (code, hint) = match error {
        OwnerWriteError::NoSigner | OwnerWriteError::NotOffered => (
            "autosync::sign::no_signer",
            "run `board --personal autosync add` from a release build of board on the owner's \
             Mac, where attention-sign shows the record for Touch ID",
        ),
        OwnerWriteError::Changes { .. } => (
            "autosync::sign::declined",
            "change the record as the owner asked, then run `board --personal autosync add` again",
        ),
        OwnerWriteError::Cancelled => (
            "autosync::sign::cancelled",
            "run `board --personal autosync add` again when the owner is ready to sign",
        ),
        OwnerWriteError::NoCache
        | OwnerWriteError::NoneNamed
        | OwnerWriteError::Unwritten { .. }
        | OwnerWriteError::Failed { .. }
        | OwnerWriteError::Bad { .. } => (
            "autosync::sign::failed",
            "check that attention-sign and the allowed signers file are installed, then run \
             `board --personal autosync add` again",
        ),
    };
    refusal(
        code,
        format!("the record was not written: {error}"),
        Map::new(),
        hint,
    )
}

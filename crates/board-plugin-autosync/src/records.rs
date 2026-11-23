//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Record, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `autosync/board/v1`: one board this Mac keeps in step with its remote,
/// at `/autosync/boards/<name>` in the personal board. autosync trusts it
/// only when `signature` is the owner's signature of the other fields.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The board's directory: absolute, or under the home as `~/...`.
    #[schemars(regex(pattern = r"^(/|~/).+"))]
    pub path: String,
    /// The branch HEAD must be on and the sync pushes, `main` when left
    /// out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^[A-Za-z0-9._/-]+$"))]
    pub branch: Option<String>,
    /// The remote URLs the board may push to. Every push URL of the remote
    /// its branch follows must be one of them.
    #[schemars(length(min = 1))]
    pub origins: Vec<String>,
    /// The owner's armored SSH signature of their put of this record
    /// without this field, made with Touch ID by `autosync add`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Record for BoardRecord {
    const TYPE: &'static str = "autosync/board/v1";
}

impl BoardRecord {
    /// The branch the sync keeps in step.
    pub(crate) fn branch(&self) -> &str {
        self.branch.as_deref().unwrap_or("main")
    }
}

/// Where the records live in the personal board.
pub(crate) const BOARDS: &str = "/autosync/boards/";

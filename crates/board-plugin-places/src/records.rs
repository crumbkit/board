//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Accepts, Actor, Record, Ref, Tag, Timestamp, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `places/workspace-change/v1`: one mxv workspace on one machine works on
/// one change, at `/places/<machine>/<workspace>/changes/<change-name>`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChange {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The workspace's directory, `/places/<machine>/<workspace>`, which
    /// holds this record's `changes/` directory.
    #[schemars(with = "Ref")]
    pub workspace: String,
    /// The change the workspace works on.
    #[schemars(with = "Ref<Change>")]
    pub change: String,
    /// The mxv workspace's id when the record was written.
    #[schemars(with = "WorkspaceId")]
    pub workspace_id: String,
    /// When the record was written.
    #[schemars(with = "Timestamp")]
    pub since: String,
    /// Who wrote the record, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Actor>,
}

impl Record for WorkspaceChange {
    const TYPE: &'static str = "places/workspace-change/v1";
}

/// A change of the changes plugin, the type a join's `change` names.
#[derive(Debug)]
pub enum Change {}

impl Accepts for Change {
    const TYPES: &'static [&'static str] = &[CHANGE_TYPE];
}

/// The record type of a change, which the changes plugin ships.
pub(crate) const CHANGE_TYPE: &str = "changes/change/v2";

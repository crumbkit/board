//! `detach`: remove the join of one workspace on this machine to one
//! change.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::attach::{found, read_join, unreadable};
use crate::board::{self, Held, logical};
use crate::mxv::{self, Listing};
use crate::{join_path, machine};

/// The change whose join to remove, and the workspace it joins.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DetachInput")]
pub(crate) struct Input {
    /// The change's logical path, such as /changes/fix-login.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    change: String,
    /// The mxv workspace's name, which needs no mxv; the workspace that holds the caller's directory when left out.
    #[schemars(length(min = 1))]
    workspace: Option<String>,
}

/// The join's path, and whether this call removed it. `removed` is false
/// when no join of the workspace to that change was there.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DetachOutput")]
pub(crate) struct Output {
    path: String,
    removed: bool,
}

/// Removes the join at `/places/<machine>/<workspace>/changes/<change-name>`
/// when it joins the workspace to the change.
pub(crate) fn detach(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { change, workspace } = PluginError::read_input("places", "detach", input)?;
    let machine = machine::read(host)?;
    let name = if let Some(name) = workspace {
        name
    } else {
        let workspaces = mxv::list(host, Listing::Live)?;
        found(host, &workspaces, None)?.name.clone()
    };
    let change = logical(&change)?;
    let path = join_path(&machine, &name, &change)?;
    let removed = match board::get(host, &path)? {
        Held::Nothing => false,
        Held::Other => return Err(unreadable(&path)),
        Held::Record(held, blob) => {
            // A join to another change of the same name is that change's
            // to remove.
            if read_join(&path, &held)?.change == change.as_str() {
                let line = json!({
                    "path": path.as_str(),
                    "expect": blob,
                    "remove": true,
                    "ignore_unmatch": true,
                });
                board::put(host, &[line])?.absent.is_empty()
            } else {
                false
            }
        }
    };
    let output = Output {
        path: path.to_string(),
        removed,
    };
    Ok(serde_json::to_value(output).expect("the output is a string and a boolean"))
}

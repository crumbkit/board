//! `covers`: the boundaries a check would apply, matched by the same rules,
//! checking and writing nothing.

use board_ops::{Host, PluginError};
use board_plugin::shape::Ref;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::boundaries::{self, Globs};
use crate::crossing::output;
use crate::personal;
use crate::records::BoundaryRecord;

/// Name the boundaries that cover the action to the destination from the
/// checkout, checking nothing.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CoversInput")]
pub(crate) struct Input {
    /// The capability that would send, such as github.pr-create.
    action: String,
    /// Where it would go, such as github:octo/board.
    to: String,
    /// The checkout's path in the personal board, such as /repos/board; left
    /// out for a checkout no board mounts.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    from: Option<String>,
}

/// The boundaries that cover it; none when nothing would be checked.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CoversOutput")]
pub(crate) struct Output {
    /// The boundaries a check would apply, by their paths, sorted.
    #[schemars(with = "Vec<Ref<BoundaryRecord>>")]
    boundaries: Vec<String>,
}

/// The boundaries in the personal board that cover the action, destination
/// and checkout, or `boundary::boundary::invalid` as `check` refuses.
pub(crate) fn covers(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { action, to, from } = PluginError::read_input("boundary", "covers", input)?;
    let personal = personal(host)?;
    let personal = &*personal;
    let every = boundaries::read(personal)?;
    let asked = (every.iter()).flat_map(|boundary| boundary.asked(&action, &to, &[]));
    let globs = Globs::answer(personal, asked)?;
    let mut boundaries: Vec<String> = (every.into_iter())
        .filter(|boundary| boundary.covers(&globs, &action, &to, from.as_deref()))
        .map(|boundary| boundary.path)
        .collect();
    boundaries.sort();
    Ok(output(&Output { boundaries }))
}

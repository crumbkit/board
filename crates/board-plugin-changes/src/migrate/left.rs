//! What the migration leaves as it is.

use board_plugin::shape::Url;
use schemars::JsonSchema;
use serde::Serialize;

/// A record the migration could not rewrite, and why, with the GitHub page
/// `github.ref` could not record when that is the reason.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct Left {
    pub(super) path: String,
    pub(super) why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Url>")]
    pub(super) page: Option<String>,
    /// The github records `github.ref` wrote for the change's earlier pages
    /// before it refused `page`. They stay, and the next run uses them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) recorded: Vec<String>,
}

/// The old change at `path`, left as it is because of `why`, with no GitHub
/// page to blame.
pub(super) fn unwritten(path: &str, why: String) -> Left {
    Left {
        path: path.to_owned(),
        why,
        page: None,
        recorded: Vec::new(),
    }
}

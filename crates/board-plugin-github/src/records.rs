//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.
//! They keep the bytes the Python plugin shipped, so a board that enabled
//! github before the port needs no `plugin enable`. `$type` comes last, as
//! it did, so `required` keeps its order.

use board_plugin::shape::{Prose, Record, Tag};
use schemars::JsonSchema;

/// `github/pull/v1`: one pull request, at `/gh/<owner>/<repo>/pulls/<number>`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullRecord {
    /// The repository's owner, a user or organisation, in lower case, such as octo.
    #[schemars(regex(pattern = "^[^A-Z]+$"))]
    pub owner: String,
    /// The repository's name, in lower case, such as widgets.
    #[schemars(regex(pattern = "^[^A-Z]+$"))]
    pub repo: String,
    /// The number GitHub gave it in the repository.
    #[schemars(range(min = 1))]
    pub number: u64,
    /// Its page on GitHub.
    #[schemars(regex(pattern = "^https://"))]
    pub url: String,
    /// Its title when the record was written; later edits on GitHub leave it as is.
    #[schemars(with = "Option<Prose>", length(min = 1))]
    pub title: Option<String>,
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
}

impl Record for PullRecord {
    const TYPE: &'static str = "github/pull/v1";
}

/// `github/issue/v1`: one issue, at `/gh/<owner>/<repo>/issues/<number>`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueRecord {
    /// The repository's owner, a user or organisation, in lower case, such as octo.
    #[schemars(regex(pattern = "^[^A-Z]+$"))]
    pub owner: String,
    /// The repository's name, in lower case, such as widgets.
    #[schemars(regex(pattern = "^[^A-Z]+$"))]
    pub repo: String,
    /// The number GitHub gave it in the repository.
    #[schemars(range(min = 1))]
    pub number: u64,
    /// Its page on GitHub.
    #[schemars(regex(pattern = "^https://"))]
    pub url: String,
    /// Its title when the record was written; later edits on GitHub leave it as is.
    #[schemars(with = "Option<Prose>", length(min = 1))]
    pub title: Option<String>,
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
}

impl Record for IssueRecord {
    const TYPE: &'static str = "github/issue/v1";
}

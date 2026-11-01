//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed. It keeps the
//! bytes the Python plugin shipped, so a board that enabled jira before the
//! port needs no `plugin enable`.

use board_plugin::shape::{Prose, Record, Tag};
use schemars::JsonSchema;

/// `jira/issue/v1`: one Jira issue, at `/jira/<site>/<key>`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The site name before .atlassian.net, such as acme.
    #[schemars(regex(pattern = r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"))]
    pub site: String,
    /// The issue key, such as ACME-123.
    #[schemars(regex(pattern = r"^[A-Z][A-Z0-9_]+-[1-9][0-9]*$"))]
    pub key: String,
    // The description names an address, which a doc comment would have to
    // mark up and so change the schema's bytes.
    #[schemars(
        description = "The address of the issue's page, such as https://acme.atlassian.net/browse/ACME-123.",
        regex(
            pattern = r"^https://[a-z0-9]([a-z0-9-]*[a-z0-9])?\.atlassian\.net/browse/[A-Z][A-Z0-9_]+-[1-9][0-9]*$"
        )
    )]
    pub url: String,
    /// The issue's summary when the record was written; absent when it could not be read.
    #[schemars(with = "Option<Prose>", length(min = 1))]
    pub title: Option<String>,
}

impl Record for IssueRecord {
    const TYPE: &'static str = "jira/issue/v1";
}

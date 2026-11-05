//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Prose, Record, Ref, Repository, Tag, Url};
use schemars::JsonSchema;

/// `topics/topic/v1`: a concept, tool, model or design that several records
/// concern, such as a library several evals test. Those records name it by
/// its path, `/topics/<slug>`, so they link to one place instead of each
/// repeating its name.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/topics/"))]
pub struct TopicRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The topic's name as people write it, such as `Raycast Workspaces`.
    #[schemars(with = "Prose", length(min = 1))]
    pub title: String,
    /// What the topic is, in words.
    #[schemars(with = "Option<Prose>", length(min = 1))]
    pub description: Option<String>,
    /// Other ways people write its name, such as `sandbox-exec` for
    /// `seatbelt`. `evals.about --subject` finds the topic by its title or
    /// any alias.
    #[schemars(with = "Option<Vec<Prose>>", length(min = 1))]
    pub aliases: Option<Vec<String>>,
    /// Pages about the topic.
    #[schemars(with = "Option<Vec<Url>>", length(min = 1))]
    pub urls: Option<Vec<String>>,
    /// The repositories that hold it, as `owner/name`.
    #[schemars(with = "Option<Vec<Repository>>", length(min = 1))]
    pub repos: Option<Vec<String>>,
    /// Other board records about it.
    #[schemars(length(min = 1))]
    pub refs: Option<Vec<Ref>>,
}

impl Record for TopicRecord {
    const TYPE: &'static str = "topics/topic/v1";
}

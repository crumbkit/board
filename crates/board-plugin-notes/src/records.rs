//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed. It keeps the
//! bytes the Python plugin shipped, with a title on every property as
//! pydantic wrote it, so a board that enabled notes before the port needs
//! no `plugin enable`.

use board_plugin::shape::{Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "RepoRecord",
    description = "`notes/repo/v1`, at `notes-repo.json` in a notes repo's root: which work the repo's notes are about.\n\n`covers` names GitHub repositories as `owner/name`, or all of an owner's\nas `owner/*`. A note from a checkout whose remote one notes repo covers\ngoes to that repo. `personal` marks the one notes repo that takes, in its\n`inbox/`, the notes no single repo covers."
)]
pub struct RepoRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(
        title = "Covers",
        schema_with = "covers",
        description = "GitHub repositories as owner/name, or owner/* for all of an owner's; an empty list covers none."
    )]
    pub covers: Vec<String>,
    #[schemars(
        title = "Personal",
        description = "Whether this repo takes, in inbox/, the notes no single repo covers."
    )]
    pub personal: Option<bool>,
}

impl Record for RepoRecord {
    const TYPE: &'static str = board_repos::RECORD_TYPE;
}

/// `covers`: each an owner and a name, or an owner and `*`.
fn covers(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {"type": "string", "pattern": "^[A-Za-z0-9-]+/(\\*|[A-Za-z0-9._-]+)$"},
    })
}

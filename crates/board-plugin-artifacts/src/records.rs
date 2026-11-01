//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.

use board_plugin::shape::{Accepts, Prose, Record, Ref, Sha256, Tag, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `artifacts/artifact/v1` at `/artifacts/<id>`: one claude.ai artifact.
///
/// `<id>` is the last segment of `url`. `first_seen` is the first publish
/// or read the mod saw, or for an import the listing row's `updatedAt`, or
/// the import time when the row has none. The version records under
/// `<id>/versions/` hold each version the board saw; the newest is the
/// artifact's current version. Only `artifacts.agree` sets `state` to
/// `agreed`, naming the owner's decision in `agreed_by`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend(
    "if" = {"properties": {"state": {"const": "agreed"}}, "required": ["state"]},
    "then" = {"required": ["agreed_by"]},
))]
pub struct ArtifactRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    #[schemars(
        description = "The artifact's claude.ai link, such as https://claude.ai/artifact/AbCdEf0123456789xyzQ.",
        with = "Url"
    )]
    pub url: String,
    /// The email address of the claude.ai account that published the artifact.
    #[schemars(length(min = 1))]
    pub account: String,
    /// When the board first saw the artifact, with a time zone.
    #[schemars(extend("format" = "date-time"))]
    pub first_seen: String,
    /// The task and change the artifact belongs to; left out when it
    /// belongs to none yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(with = "Vec<Ref<Work>>")]
    pub subject: Vec<String>,
    /// One sentence saying what the page is for; left out until an agent
    /// sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>", length(min = 1))]
    pub shows: Option<String>,
    /// Where the owner's review stands: draft until it is shown, shown once
    /// the owner has seen it, agreed once the owner's decision accepts it,
    /// and superseded once a newer artifact replaces it.
    pub state: State,
    /// The owner's decision that moved the artifact to agreed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<Decision>>")]
    pub agreed_by: Option<String>,
    /// The artifact that replaces this one, once it is superseded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Ref<ArtifactRecord>>")]
    pub superseded_by: Option<String>,
    /// Tags that place the artifact among neighbouring work, such as
    /// dashboard/flows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(inner(regex(pattern = r"^[a-z0-9]+(-[a-z0-9]+)*(/[a-z0-9]+(-[a-z0-9]+)*)*$")))]
    pub area: Vec<String>,
}

impl Record for ArtifactRecord {
    const TYPE: &'static str = "artifacts/artifact/v1";
}

/// Where the owner's review of an artifact stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum State {
    Draft,
    Shown,
    Agreed,
    Superseded,
}

/// The work an artifact belongs to: a change or a task.
#[derive(Debug)]
pub enum Work {}

impl Accepts for Work {
    const TYPES: &'static [&'static str] = &["changes/change/v1", "tasks/task/v2"];
}

/// The plans plugin's decision record, which `agreed_by` names.
#[derive(Debug)]
pub enum Decision {}

impl Record for Decision {
    const TYPE: &'static str = "plans/decision/v1";
}

/// The answer record a decision of the owner's names.
#[derive(Debug)]
pub enum Answer {}

impl Record for Answer {
    const TYPE: &'static str = "plans/answer/v1";
}

/// `artifacts/version/v1` at `/artifacts/<id>/versions/<key>`: one version,
/// written once.
///
/// The page is beside it as `<key>.html` or `<key>.md`. `<key>` is
/// `version`, the version id the Artifact tool returned, or, when it
/// returned none, the first 16 hex digits of `sha256`, and the record leaves
/// `version` out. `title` and `description` are the artifact's at this
/// version. A version with no local page, such as one an Artifact type
/// wrote, has no copy and says `copy: "none"`, and then has no `sha256`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend(
    "if" = {"required": ["copy"]},
    "then" = {"not": {"required": ["sha256"]}},
    "else" = {"required": ["sha256"]},
))]
pub struct VersionRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The version id the Artifact tool returned, such as 1793473476-1c42.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = "^[A-Za-z0-9_-]+$"))]
    pub version: Option<String>,
    /// The artifact's title at this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub title: Option<String>,
    /// The description the publish passed, when it passed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub description: Option<String>,
    /// The SHA-256 of the page copied beside this record, in hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Sha256>")]
    pub sha256: Option<String>,
    /// The local file the publish sent, when a publish wrote this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub source: Option<String>,
    /// The Claude Code session that published or read this version.
    #[schemars(length(min = 1))]
    pub session: String,
    /// When the session published or read this version, with a time zone.
    #[schemars(extend("format" = "date-time"))]
    pub at: String,
    /// none when the version has no local page, so nothing was copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<NoCopy>,
}

impl Record for VersionRecord {
    const TYPE: &'static str = "artifacts/version/v1";
}

/// A version with no copy of its page.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum NoCopy {
    None,
}

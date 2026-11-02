//! `artifacts.import`: an artifact record for each artifact of an Artifact
//! tool listing that the board does not have.

use board_ops::{Absent, Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::held::{self, logical, refused, timestamp};
use crate::list::LIST_LIMIT;
use board_plugin::shape::Tag;

use crate::records::{ArtifactRecord, State};

/// A claude.ai artifact's link. Its last segment names the artifact's
/// records, so it holds no `/` or `.`.
pub(crate) const ARTIFACT_URL: &str = r"^https://claude\.ai/(code/)?artifact/[A-Za-z0-9_-]+$";

/// Record each artifact of an Artifact tool listing that the board does not have.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ImportInput")]
pub(crate) struct Input {
    /// The email address of the claude.ai account that published the artifacts.
    #[schemars(length(min = 1))]
    account: String,
    /// The Artifact tool's list result, unchanged. Board reads stdin only
    /// when no flag is given, so give the whole input as JSON on stdin:
    /// {"account": "owner@example.com", "listing": {...}}.
    listing: Listing,
}

/// The Artifact tool's `list` result, unchanged: `{artifacts, truncated?,
/// total?}` and the tool's other fields.
///
/// Only a listing of the account's own artifacts is admitted, so a `scope`
/// of `shared` or `all` is refused.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Listing {
    /// The listed artifacts, at most 200, most recently updated first.
    #[schemars(length(max = 200))]
    artifacts: Vec<Row>,
    /// Whether the account has more artifacts than the listing holds.
    truncated: Option<bool>,
    /// How many artifacts the account has.
    #[schemars(range(min = 0))]
    total: Option<u64>,
    /// Absent in a listing of the account's own artifacts; mine is the same.
    #[serde(rename = "scope")]
    _scope: Option<Mine>,
}

/// The one scope a listing may name.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Mine {
    Mine,
}

/// One artifact of the Artifact tool's list result. The tool's other fields
/// are kept out of the record.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Row {
    #[schemars(
        description = "The artifact's claude.ai link, such as https://claude.ai/artifact/AbCdEf0123456789xyzQ.",
        regex(pattern = ARTIFACT_URL)
    )]
    url: String,
    /// The artifact's title in the listing.
    #[serde(rename = "title")]
    _title: String,
    /// When the artifact was last updated, with a time zone.
    #[serde(rename = "updatedAt")]
    #[schemars(extend("format" = "date-time"))]
    updated_at: Option<String>,
}

/// The artifact records written, by path, how many the board already had,
/// and whether the listing was cut short.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ImportOutput")]
pub(crate) struct Output {
    written: Vec<String>,
    known: usize,
    truncated: Option<Truncated>,
}

/// A listing that left artifacts out: how many it listed, how many the
/// account has, and what that means.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Truncated {
    listed: usize,
    total: Option<u64>,
    message: String,
}

/// The last segment of an artifact's link, which names its records.
pub(crate) fn artifact_id(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// Writes, in one commit, an artifact record at `/artifacts/<id>` for each
/// listed link that has none. `first_seen` is the row's `updatedAt`, or the
/// time of the import when the row has none. A path that holds anything
/// already is left as it is. The records carry no version: reading each new
/// artifact lets the mod record it.
pub(crate) fn import(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { account, listing } = PluginError::read_input("artifacts", "import", input)?;
    let rows: Vec<(String, &Row)> = (listing.artifacts.iter())
        .map(|row| (format!("/artifacts/{}", artifact_id(&row.url)), row))
        .collect();
    let paths = (rows.iter())
        .map(|(path, _)| logical(path))
        .collect::<Result<Vec<_>, _>>()?;
    let answers =
        (host.refs(&paths, Absent::Refuse)).map_err(|error| refused("refs", &error.refusal()))?;
    let now = timestamp(host.settings().now);
    let mut written = Vec::new();
    let mut lines = Vec::new();
    for ((path, row), answer) in rows.iter().zip(answers) {
        if answer.is_ok() {
            continue;
        }
        let record = ArtifactRecord {
            tag: Tag::default(),
            url: row.url.clone(),
            account: account.clone(),
            first_seen: row.updated_at.clone().unwrap_or_else(|| now.clone()),
            subject: Vec::new(),
            shows: None,
            state: State::Draft,
            agreed_by: None,
            superseded_by: None,
            area: Vec::new(),
        };
        lines.push(json!({"path": path, "expect": "none", "record": record}));
        written.push(path.clone());
    }
    if !lines.is_empty() {
        held::put(host, &lines)?;
    }
    let output = Output {
        known: rows.len() - written.len(),
        written,
        truncated: truncated(&account, &listing),
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and numbers"))
}

/// What a listing that says it is truncated left out, or none for a whole
/// listing.
fn truncated(account: &str, listing: &Listing) -> Option<Truncated> {
    if listing.truncated != Some(true) {
        return None;
    }
    let listed = listing.artifacts.len();
    let of = (listing.total).map_or_else(String::new, |total| format!("of {total} "));
    Some(Truncated {
        listed,
        total: listing.total,
        message: format!(
            "the listing holds the {listed} most recently updated {of}artifacts of {account}, and \
             the Artifact tool lists no more than {LIST_LIMIT}, so the rest are not imported; a \
             regular sync catches each one when it is updated"
        ),
    })
}

//! `artifacts.record`: the artifact record the mod writes when a session
//! publishes or reads an artifact, and what the agent still owes it.

use std::collections::BTreeSet;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::held::{self, timestamp};
use crate::import::{ARTIFACT_URL, artifact_id};
use crate::list::about;
use board_plugin::shape::Tag;

use crate::records::{ArtifactRecord, State};

/// The artifact a session published or read, and the work the session
/// belongs to.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RecordInput")]
pub(crate) struct Input {
    #[schemars(
        description = "The artifact's claude.ai link, such as https://claude.ai/artifact/AbCdEf0123456789xyzQ.",
        regex(pattern = ARTIFACT_URL)
    )]
    url: String,
    /// The email address of the claude.ai account the session is signed in to.
    #[schemars(length(min = 1))]
    account: String,
    /// The change or task the publishing session works on, such as
    /// /changes/flows-editor; left out for a read or when the session names
    /// none.
    #[serde(default)]
    #[schemars(inner(regex(pattern = "^/")))]
    subject: Vec<String>,
}

/// The artifact's record after the call, whether the call wrote it, and
/// the artifacts in `shown` or `agreed` that share its subject, which a new
/// artifact may replace.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RecordOutput")]
pub(crate) struct Output {
    path: String,
    created: bool,
    subject: Vec<String>,
    shows: Option<String>,
    state: State,
    may_replace: Vec<String>,
}

/// Writes the artifact record when it is missing, with the subject given
/// and in `draft`, or gives a held record with no subject the one given.
pub(crate) fn record(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        url,
        account,
        subject,
    } = PluginError::read_input("artifacts", "record", input)?;
    let path = format!("/artifacts/{}", artifact_id(&url));
    let (record, created) = match held::held(host, &path)? {
        None => {
            let record = ArtifactRecord {
                tag: Tag::default(),
                url,
                account,
                first_seen: timestamp(host.settings().now),
                subject,
                shows: None,
                state: State::Draft,
                agreed_by: None,
                superseded_by: None,
                area: Vec::new(),
            };
            held::put(
                host,
                &[json!({"path": path, "expect": "none", "record": record})],
            )?;
            (record, true)
        }
        Some((mut record, blob)) if record.subject.is_empty() && !subject.is_empty() => {
            record.subject = subject;
            held::put(
                host,
                &[json!({"path": path, "expect": blob, "record": record})],
            )?;
            (record, false)
        }
        Some((record, _)) => (record, false),
    };
    let may_replace = if created {
        replaced(host, &path, &record.subject)?
    } else {
        Vec::new()
    };
    let output = Output {
        path,
        created,
        subject: record.subject,
        shows: record.shows,
        state: record.state,
        may_replace,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and lists"))
}

/// The other artifacts in `shown` or `agreed` whose subject reaches one of
/// `subject`, by path.
fn replaced(host: &dyn Host, path: &str, subject: &[String]) -> Result<Vec<String>, PluginError> {
    if subject.is_empty() {
        return Ok(Vec::new());
    }
    let mut sharing = BTreeSet::new();
    // A subject that names nothing yet shares no artifact; the put has
    // already written it and Board reports it as dangling.
    for work in subject {
        if let Ok(paths) = about(host, work) {
            sharing.extend(paths);
        }
    }
    sharing.remove(path);
    let held = held::all(host)?;
    Ok((held.held.into_iter())
        .filter(|held| {
            sharing.contains(&held.path)
                && matches!(held.record.state, State::Shown | State::Agreed)
        })
        .map(|held| held.path)
        .collect())
}

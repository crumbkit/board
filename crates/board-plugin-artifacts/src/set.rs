//! `artifacts.set`: fill an artifact's subject, shows, area and state, and
//! mark the artifact it replaces superseded.

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::held::{self, refusal};
use crate::records::{ArtifactRecord, State};

/// The artifact and the fields to fill. `subject` and `area` add to what
/// the record holds; `shows` replaces it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SetInput")]
pub(crate) struct Input {
    /// The artifact's record, such as /artifacts/AbCdEf0123456789xyzQ.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// A change or task the artifact belongs to, such as /changes/flows-editor.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    subject: Option<String>,
    /// One sentence saying what the page is for.
    #[schemars(length(min = 1))]
    shows: Option<String>,
    /// An area tag, such as dashboard/flows.
    #[schemars(regex(pattern = r"^[a-z0-9]+(-[a-z0-9]+)*(/[a-z0-9]+(-[a-z0-9]+)*)*$"))]
    area: Option<String>,
    /// shown once the owner has seen the artifact, or draft. Only
    /// artifacts.agree moves an artifact to agreed.
    state: Option<Settable>,
    /// The artifact this one replaces, which becomes superseded.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    supersedes: Option<String>,
}

/// The states `set` may give an artifact.
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Settable {
    Draft,
    Shown,
}

/// The artifact's fields after the call, and the artifact it superseded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SetOutput")]
pub(crate) struct Output {
    path: String,
    subject: Vec<String>,
    shows: Option<String>,
    state: State,
    area: Vec<String>,
    superseded: Option<String>,
}

/// Fills the fields given, in one commit with the superseded artifact.
pub(crate) fn set(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input("artifacts", "set", input)?;
    let path = asked.path.as_str();
    if asked.subject.is_none()
        && asked.shows.is_none()
        && asked.area.is_none()
        && asked.state.is_none()
        && asked.supersedes.is_none()
    {
        return Err(refusal(
            "artifacts::set::nothing",
            format!(
                "set was given nothing to change on {path}; pass --subject, --shows, --area, \
                 --state or --supersedes"
            ),
            &[("path", path)],
        ));
    }
    let (mut record, blob) = held::artifact(host, path)?;
    if let Some(state) = asked.state {
        settable(path, &record)?;
        record.state = match state {
            Settable::Draft => State::Draft,
            Settable::Shown => State::Shown,
        };
    }
    if let Some(subject) = asked.subject
        && !record.subject.contains(&subject)
    {
        record.subject.push(subject);
    }
    if let Some(area) = asked.area
        && !record.area.contains(&area)
    {
        record.area.push(area);
    }
    if asked.shows.is_some() {
        record.shows = asked.shows;
    }
    let mut lines = vec![json!({"path": path, "expect": blob, "record": record})];
    if let Some(old) = &asked.supersedes {
        let (mut replaced, blob) = held::artifact(host, old)?;
        replaced.state = State::Superseded;
        replaced.superseded_by = Some(path.to_owned());
        lines.push(json!({"path": old, "expect": blob, "record": replaced}));
    }
    held::put(host, &lines)?;
    let output = Output {
        path: asked.path.clone(),
        subject: record.subject,
        shows: record.shows,
        state: record.state,
        area: record.area,
        superseded: asked.supersedes,
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and lists"))
}

/// Refuses a state change on an artifact that is agreed or superseded,
/// whose state only a decision or a newer artifact gives.
fn settable(path: &str, record: &ArtifactRecord) -> Result<(), PluginError> {
    let state = match record.state {
        State::Draft | State::Shown => return Ok(()),
        State::Agreed => "agreed",
        State::Superseded => "superseded",
    };
    Err(refusal(
        "artifacts::state::settled",
        format!(
            "{path} is {state}, and set gives only draft or shown to an artifact that is neither; \
             publish a new artifact and pass its path with --supersedes {path}"
        ),
        &[("path", path), ("state", state)],
    ))
}

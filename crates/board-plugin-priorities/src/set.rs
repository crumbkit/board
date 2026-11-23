//! `set`, `move` and `drop`: add, replace, reorder and remove entries. Each
//! checks its input against the record first and writes nothing when it
//! refuses.

use board_model::{Code, LogicalPath};
use board_ops::{Host, PluginError};
use board_plugin::shape::{Summary, whole};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::{COVERED_TYPES, Entry, Horizon, MOST_COVERS, MOST_ENTRIES, PrioritiesRecord};
use crate::{held, not_found, refusal, refused, write};

/// The entry to add or replace.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SetInput")]
pub(crate) struct Input {
    /// short for this week, long for beyond it. An entry in the other
    /// horizon moves here.
    horizon: Horizon,
    /// The entry's id: lowercase letters and digits in words joined by
    /// `-`, such as `board-release`. An entry with this id is replaced.
    id: String,
    /// What the priority is, in one line of at most 25 words.
    #[schemars(with = "Summary")]
    title: String,
    /// The tasks and changes in this board the entry covers, on stdin. Left
    /// out, a replaced entry keeps its own and a new entry covers nothing.
    #[schemars(inner(regex(pattern = "^/")))]
    covers: Option<Vec<String>>,
    /// The rank to put the entry at, 1 for the top. Left out, a replaced
    /// entry keeps its place in its horizon and any other goes last. A rank
    /// past the end puts it last.
    #[serde(default, deserialize_with = "optional_whole")]
    #[schemars(range(min = 1, max = 10000))]
    at: Option<u16>,
}

/// The entry to move and where.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MoveInput")]
pub(crate) struct MoveInput {
    /// The entry's id.
    id: String,
    /// The rank to move it to in its horizon, 1 for the top. A rank past
    /// the end puts it last.
    #[serde(deserialize_with = "whole")]
    #[schemars(range(min = 1, max = 10000))]
    to: u16,
}

/// The entry to remove.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DropInput")]
pub(crate) struct DropInput {
    /// The entry's id.
    id: String,
}

/// The entry the call changed, where it now is, and whether the owner has
/// still to confirm it. For `drop`, the rank it had.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ChangedOutput")]
pub(crate) struct Output {
    id: String,
    horizon: Horizon,
    rank: usize,
    unconfirmed: bool,
}

fn optional_whole<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u16>, D::Error> {
    whole(deserializer).map(Some)
}

/// Adds the entry, or replaces the one with its id.
pub(crate) fn set(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input("priorities", "set", input)?;
    is_id(&asked.id)?;
    let (mut record, blob) = held(host)?;
    if let Some(covers) = &asked.covers {
        covered(host, covers)?;
    }
    let found = record.find(&asked.id);
    let staying = found.is_some_and(|(horizon, _)| horizon == asked.horizon);
    let list = record.entries(asked.horizon);
    if !staying && list.len() >= MOST_ENTRIES {
        return Err(full(asked.horizon, list));
    }
    let old = found.map(|(horizon, index)| record.entries_mut(horizon).remove(index));
    let covers = (asked.covers.clone())
        .or_else(|| old.as_ref().map(|old| old.covers.clone()))
        .unwrap_or_default();
    // The owner confirmed the words and the covers, so a change to either
    // leaves the entry to confirm again.
    let confirmed = old
        .filter(|old| old.title == asked.title && old.covers == covers)
        .and_then(|old| old.confirmed);
    let entry = Entry {
        id: asked.id,
        title: asked.title,
        covers,
        confirmed,
    };
    let list = record.entries_mut(asked.horizon);
    let index = match (asked.at, found) {
        (Some(at), _) => usize::from(at) - 1,
        (None, Some((horizon, index))) if horizon == asked.horizon => index,
        (None, _) => list.len(),
    }
    .min(list.len());
    list.insert(index, entry);
    finish(host, &record, &blob, asked.horizon, index)
}

/// Moves an entry to another rank in its horizon.
pub(crate) fn reorder(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: MoveInput = PluginError::read_input("priorities", "move", input)?;
    let (mut record, blob) = held(host)?;
    let (horizon, index) = record
        .find(&asked.id)
        .ok_or_else(|| not_found(&record, &asked.id))?;
    let list = record.entries_mut(horizon);
    let entry = list.remove(index);
    let to = (usize::from(asked.to) - 1).min(list.len());
    list.insert(to, entry);
    finish(host, &record, &blob, horizon, to)
}

/// Removes an entry.
pub(crate) fn remove(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: DropInput = PluginError::read_input("priorities", "drop", input)?;
    let (mut record, blob) = held(host)?;
    let (horizon, index) = record
        .find(&asked.id)
        .ok_or_else(|| not_found(&record, &asked.id))?;
    let entry = record.entries_mut(horizon).remove(index);
    write(host, &record, &blob)?;
    let output = Output {
        id: entry.id,
        horizon,
        rank: index + 1,
        unconfirmed: entry.confirmed.is_none(),
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and numbers"))
}

/// Writes `record` and reports the entry at `index` in `horizon`.
fn finish(
    host: &dyn Host,
    record: &PrioritiesRecord,
    blob: &str,
    horizon: Horizon,
    index: usize,
) -> Result<Value, PluginError> {
    write(host, record, blob)?;
    let entry = &record.entries(horizon)[index];
    let output = Output {
        id: entry.id.clone(),
        horizon,
        rank: index + 1,
        unconfirmed: entry.confirmed.is_none(),
    };
    Ok(serde_json::to_value(output).expect("the output is plain strings and numbers"))
}

/// Refuses an id that is not a slug.
fn is_id(id: &str) -> Result<(), PluginError> {
    let slug = !id.is_empty()
        && (id.split('-')).all(|word| {
            !word.is_empty()
                && (word.bytes()).all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if slug {
        return Ok(());
    }
    let mut refused = refusal(
        "priorities::id::invalid",
        format!(
            "the id {id:?} is not lowercase letters and digits in words joined by `-`, such as \
             board-release"
        ),
        &[("id", id)],
    );
    refused.hint = Some("pass an id such as board-release".to_owned());
    Err(refused)
}

/// Refuses an entry for a horizon that holds [`MOST_ENTRIES`], naming its
/// lowest-ranked entry.
fn full(horizon: Horizon, list: &[Entry]) -> PluginError {
    let lowest = list.last().map_or("", |entry| entry.id.as_str());
    let name = horizon.name();
    let mut refused = refusal(
        "priorities::horizon::full",
        format!(
            "the {name} horizon holds {MOST_ENTRIES} entries, the most it may; its lowest-ranked \
             is {lowest}"
        ),
        &[("horizon", name), ("lowest", lowest)],
    );
    refused.hint = Some(format!(
        "merge related entries, move one to the other horizon, or drop one, such as with \
         `board priorities drop --id {lowest}`, and ask the owner which"
    ));
    refused
}

/// Refuses covers that are too many, that reach through a mount, or that
/// name no task or change.
fn covered(host: &dyn Host, covers: &[String]) -> Result<(), PluginError> {
    if covers.len() > MOST_COVERS {
        let count = covers.len().to_string();
        let mut refused = refusal(
            "priorities::covers::too_many",
            format!("covers names {count} records, and an entry covers at most {MOST_COVERS}"),
            &[("count", &count)],
        );
        refused.hint = Some("cover the project or parent task that holds them instead".to_owned());
        return Err(refused);
    }
    let wrong = |path: &str, what: &str| {
        let mut refused = refusal(
            "priorities::covers::wrong_type",
            format!(
                "{path} {what}; an entry covers only {} records",
                COVERED_TYPES.join(", ")
            ),
            &[("path", path)],
        );
        refused.hint = Some("cover the task or change itself".to_owned());
        refused
    };
    let paths: Vec<LogicalPath> = (covers.iter())
        .map(|path| path.parse().map_err(|_| wrong(path, "is not a board path")))
        .collect::<Result<_, _>>()?;
    let answers = (host.get_each(&paths)).map_err(|error| refused("get", &error.refusal("get")))?;
    for (path, answer) in covers.iter().zip(answers) {
        let report = match answer {
            Ok(report) => report,
            Err(error) if [Code::PathNotFound, Code::RecordNotARecord].contains(&error.code()) => {
                return Err(wrong(path, "names no record"));
            }
            Err(error) => return Err(refused("get", &error.refusal("get"))),
        };
        if let Some(mount) = report.location.mounts().first() {
            let mount = mount.to_string();
            let mut refused = refusal(
                "priorities::covers::through_mount",
                format!(
                    "{path} is in the board mounted at {mount}, and an entry covers only \
                     records in its own board"
                ),
                &[("path", path), ("mount", &mount)],
            );
            refused.hint = Some(
                "cover a local task that stands for the work, or leave covers empty".to_owned(),
            );
            return Err(refused);
        }
        let record_type =
            (report.record.ok()).and_then(|record| record["$type"].as_str().map(str::to_owned));
        match record_type {
            Some(found) if COVERED_TYPES.contains(&found.as_str()) => {}
            Some(found) => return Err(wrong(path, &format!("is a {found} record"))),
            None => return Err(wrong(path, "is not a record")),
        }
    }
    Ok(())
}

//! The priorities plugin built into Board (plugins/priorities/README.md):
//! the owner's priorities as one record at `/priorities`, ranked in a
//! short and a long horizon. `list` reads them with what each covers and
//! whether each needs confirming; `set`, `move`, `drop` and `confirm` each
//! write the record in one put that expects the blob they read.

mod confirm;
mod list;
pub mod records;
mod set;

use board_model::{CapabilityName, Code, LogicalPath, Object, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use records::{Horizon, PrioritiesRecord};

const PLUGIN: &str = "priorities";

/// Where a board keeps its priorities record.
pub const PATH: &str = "/priorities";

#[derive(Clone, Copy, Debug)]
pub struct Priorities;

impl Plugin for Priorities {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (PrioritiesRecord::TYPE.parse())
            .expect("PrioritiesRecord's TYPE is a record type; a test parses it");
        vec![(
            name,
            schema_of::<PrioritiesRecord>(SchemaSettings::draft2020_12()),
        )]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "list" => list::list(host, &input),
            "set" => set::set(host, &input),
            "move" => set::reorder(host, &input),
            "drop" => set::remove(host, &input),
            "confirm" => confirm::confirm(host, &input),
            other => Err(refusal(
                "priorities::call::unknown_capability",
                format!(
                    "priorities offers no capability {other}; it offers list, set, move, drop \
                     and confirm"
                ),
                &[],
            )),
        }
    }
}

fn manifest() -> Value {
    let settings = SchemaSettings::draft2020_12;
    let read_only = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    });
    let writes = |destructive: bool| {
        json!({
            "readOnlyHint": false,
            "destructiveHint": destructive,
            "idempotentHint": true,
            "openWorldHint": false,
        })
    };
    let capability = |name: &str, description: &str, input: Value, output: Value, hints| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": input,
            "outputSchema": output,
            "annotations": hints,
            "_meta": {},
        })
    };
    let changed = schema_of::<set::Output>(settings().for_serialize());
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "The owner's priorities, ranked in a short and a long horizon, in one \
            record at /priorities. Use it to say what the owner's priorities are, and to keep \
            them current. Start with `board priorities list`, which gives each entry's rank, \
            what it covers and whether the owner must confirm it. SKILL.md and README.md in the \
            plugin's directory say more.",
        "metadata": {"area": "planning"},
        "types": [PrioritiesRecord::TYPE],
        "capabilities": [
            capability(
                "list",
                "Each entry in rank order with what it covers, and whether it is unconfirmed or \
                 stale.",
                schema_of::<list::Input>(settings()),
                schema_of::<list::Output>(settings().for_serialize()),
                read_only,
            ),
            capability(
                "set",
                "Add an entry or replace one by its id, at a rank or in its place. A new title \
                 or covers leaves it unconfirmed.",
                schema_of::<set::Input>(settings()),
                changed.clone(),
                writes(false),
            ),
            capability(
                "move",
                "Move an entry to another rank in its horizon.",
                schema_of::<set::MoveInput>(settings()),
                changed.clone(),
                writes(false),
            ),
            capability(
                "drop",
                "Remove an entry.",
                schema_of::<set::DropInput>(settings()),
                changed,
                writes(true),
            ),
            capability(
                "confirm",
                "Record that the owner confirmed an entry, or every entry, today.",
                schema_of::<confirm::Input>(settings()),
                schema_of::<confirm::Output>(settings().for_serialize()),
                writes(false),
            ),
        ],
    })
}

/// A refusal of the plugin's own, with `fields`.
fn refusal(code: &str, message: String, fields: &[(&str, &str)]) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.iter())
            .map(|(name, value)| ((*name).to_owned(), Value::from(*value)))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// `priorities::board::refused`, for Board's refusal of `board <command>`.
fn refused(command: &str, refusal: &board_model::Refusal) -> PluginError {
    PluginError::board_refused(PLUGIN, command, refusal)
}

/// The record at [`PATH`] and the blob a write expects there, `none` when
/// the board holds no record yet.
fn held(host: &dyn Host) -> Result<(PrioritiesRecord, String), PluginError> {
    let path: LogicalPath = PATH.parse().expect("PATH is a logical path");
    let answers =
        (host.get_each(&[path])).map_err(|error| refused("get", &error.refusal("get")))?;
    let report = match answers.into_iter().next() {
        Some(Ok(report)) => report,
        Some(Err(error)) if error.code() == Code::PathNotFound => {
            return Ok((PrioritiesRecord::default(), "none".to_owned()));
        }
        Some(Err(error)) => return Err(refused("get", &error.refusal("get"))),
        None => return Ok((PrioritiesRecord::default(), "none".to_owned())),
    };
    let Object::Blob(blob) = &report.location.object else {
        return Err(unreadable("is a directory"));
    };
    let record = report.record.map_err(|refusal| refused("get", &refusal))?;
    let record = PrioritiesRecord::deserialize(&record).map_err(|error| {
        unreadable(&format!(
            "is not a priorities/list/v1 this plugin reads: {error}"
        ))
    })?;
    Ok((record, blob.to_string()))
}

fn unreadable(why: &str) -> PluginError {
    let mut refused = refusal(
        "priorities::record::unreadable",
        format!("{PATH} {why}"),
        &[("path", PATH)],
    );
    refused.hint = Some(format!(
        "run `board check {PATH}` to see what is wrong, and fix or remove the record"
    ));
    refused
}

/// Writes `record` at [`PATH`], expecting `blob` there, in one commit.
fn write(host: &dyn Host, record: &PrioritiesRecord, blob: &str) -> Result<(), PluginError> {
    let line = json!({"path": PATH, "expect": blob, "record": record});
    (host.put(&[line]))
        .map(|_| ())
        .map_err(|error| refused("put", &error.refusal()))
}

/// Today's UTC day, such as 2026-11-23.
fn today(host: &dyn Host) -> String {
    board_repos::today(host.settings().now, 0, std::time::Duration::ZERO).to_string()
}

/// Refuses an `id` that no entry has, naming the ids there are.
fn not_found(record: &PrioritiesRecord, id: &str) -> PluginError {
    let ids: Vec<&str> = (Horizon::BOTH.iter())
        .flat_map(|horizon| {
            record
                .entries(*horizon)
                .iter()
                .map(|entry| entry.id.as_str())
        })
        .collect();
    let mut refused = refusal(
        "priorities::id::not_found",
        format!(
            "no entry has the id {id}; the entries are {}",
            if ids.is_empty() {
                "none".to_owned()
            } else {
                ids.join(", ")
            }
        ),
        &[("id", id)],
    );
    refused.hint = Some("run `board priorities list` for each entry's id".to_owned());
    refused
}

#[cfg(test)]
mod tests;

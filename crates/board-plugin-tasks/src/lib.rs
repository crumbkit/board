//! The tasks plugin built into Board (plugins/tasks/README.md): it sweeps,
//! adds and writes tasks, and lists the trees their parents make, in the
//! notes repos a board holds or mounts. Routing, the sweep and the edit
//! loop are its own; reading, writing, syncing and pinning a repo are
//! board-repos'.

mod available;
mod claim;
mod dates;
mod lines;
mod listing;
mod migrate;
pub mod records;
mod refs;
mod repeat;
mod task;
mod tree;
mod trees;
mod write;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Origin, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use board_repos::{Through, Writer};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{TaskRecord, V2_SCHEMA, V2_TYPE};

/// The plugin's name, which its codes start with.
const PLUGIN: &str = "tasks";

#[derive(Clone, Copy, Debug)]
pub struct Tasks;

/// The capabilities that change an existing task. `add` writes a new
/// task, a second one on a repeat, and `done` defers a repeating task
/// again on each call.
const EDITS: [&str; 8] = [
    "claim", "done", "defer", "drop", "reviewed", "move", "ref", "migrate",
];
/// What every write call but `done` and `move` says after its own
/// description; `done` says it in its own words, since it may not push.
const WRITES: &str = " It syncs the task's repo first and pushes after, and sets reviewed.";

/// Each capability's name and description, in the order the manifest
/// lists them.
const CAPABILITIES: [(&str, &str); 11] = [
    (
        "available",
        "Sync the repos the caller's remote chooses, then list their open tasks available now, \
         those due within seven days, the stale ones, and those an owner holds.",
    ),
    (
        "tree",
        "List a task and its descendants in every status, nested by title, or with --root every \
         top-level tree in the repos the caller's remote chooses, read at their pins.",
    ),
    (
        "add",
        "Write a new open task to tasks/<slug>.md in the repo the caller's remote routes to, and \
         say in `about` which repo took it and whether it left this Mac; --repo / needs --reason.",
    ),
    (
        "claim",
        "Claim an open task for an owner, refused when another owner holds it.",
    ),
    (
        "done",
        "Mark a task done, or defer a repeating one to its next occurrence. It syncs the task's \
         repo first and pushes after, unless --push false makes it commit in the clone alone, and \
         sets reviewed.",
    ),
    (
        "defer",
        "Defer a task until a date or a horizon from today, written as a date.",
    ),
    ("drop", "Mark a task dropped."),
    ("reviewed", "Mark a task reviewed today."),
    (
        "move",
        "Set a task's parent to another task in its repo, or clear it with none, refused when the \
         parent is the task or under it. It syncs the task's repo first and pushes after, and \
         changes nothing else.",
    ),
    (
        "ref",
        "Add a record or file in a tasks/task/v3 task's own repository to the end of its refs, or \
         remove one, refused when the path holds nothing or is reached through a mount. It syncs \
         the task's repo first and pushes after, sets reviewed, and writes nothing when the refs \
         stay as they were.",
    ),
    (
        "migrate",
        "List each tasks/task/v2 task that is not a project and the tasks/task/v3 record it \
         becomes, and with --yes rewrite them, one commit per repo, each synced first and \
         pushed after; refused while a v2 project task remains.",
    ),
];

impl Plugin for Tasks {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = |text: &str| {
            text.parse()
                .expect("TYPE is a record type; a test parses it")
        };
        let settings = SchemaSettings::draft2020_12;
        vec![
            (
                name(V2_TYPE),
                serde_json::from_str(V2_SCHEMA).expect("a committed schema; a test reads it"),
            ),
            (name(TaskRecord::TYPE), schema_of::<TaskRecord>(settings())),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "available" => available::available(host, &input),
            "tree" => trees::tree(host, &input),
            "add" => write::add(host, &input),
            "claim" => claim::claim(host, &input),
            "done" => write::done(host, &input),
            "defer" => write::defer(host, &input),
            "drop" => write::drop(host, &input),
            "reviewed" => write::reviewed(host, &input),
            "move" => write::move_task(host, &input),
            "ref" => refs::ref_task(host, &input),
            "migrate" => migrate::migrate(host, &input),
            other => Err(refusal(
                "tasks::call::unknown_capability",
                format!(
                    "tasks offers no capability {other}; it offers available, tree, add, claim, \
                     done, defer, drop, reviewed, move, ref and migrate"
                ),
                [],
            )),
        }
    }
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal<'a>(
    code: &str,
    message: String,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// [`refusal`] with `hint` as its next step.
pub(crate) fn hinted<'a>(
    code: &str,
    message: String,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
    hint: &str,
) -> PluginError {
    PluginError {
        hint: Some(hint.to_owned()),
        ..refusal(code, message, fields)
    }
}

/// What a call writes through: the host, its clones, and git's deadline on
/// each sync and pin.
fn writer<'a>(host: &'a dyn Host, clones: &'a Through<'a>) -> Writer<'a> {
    Writer {
        host,
        clones,
        plugin: PLUGIN,
        deadline: board_tree::GIT_DEADLINE,
    }
}

/// The origin of the git repository Board was started in, and why there
/// is none inside a repository. Both are none outside a git repository, and
/// in the board's own repository when it has no origin.
fn caller_origin(host: &dyn Host) -> (Option<String>, Option<String>) {
    match host.caller_remote() {
        Origin::Url(url) => (Some(url), None),
        Origin::Outside => (None, None),
        Origin::Unread(_) if host.caller_in_board() => (None, None),
        Origin::Unread(why) => (None, Some(why)),
    }
}

/// The remote and the unread origin a read call chooses its repos by:
/// `remote` when given, else the caller's origin, unless `repo` names the
/// repo.
fn chosen_by(
    host: &dyn Host,
    repo: Option<&str>,
    remote: Option<&str>,
) -> (Option<String>, Option<String>) {
    match (repo, remote) {
        (None, None) => caller_origin(host),
        (_, remote) => (remote.map(str::to_owned), None),
    }
}

fn manifest() -> Value {
    let input = SchemaSettings::draft2020_12;
    let output = || SchemaSettings::draft2020_12().for_serialize();
    let written = || schema_of::<write::Written>(output());
    let schemas = [
        (
            schema_of::<available::Input>(input()),
            schema_of::<available::Output>(output()),
        ),
        (
            schema_of::<trees::Input>(input()),
            schema_of::<trees::Output>(output()),
        ),
        (
            schema_of::<write::AddInput>(input()),
            schema_of::<write::Added>(output()),
        ),
        (schema_of::<claim::ClaimInput>(input()), written()),
        (schema_of::<write::DoneInput>(input()), written()),
        (schema_of::<write::DeferInput>(input()), written()),
        (schema_of::<write::TaskPath>(input()), written()),
        (schema_of::<write::TaskPath>(input()), written()),
        (
            schema_of::<write::MoveInput>(input()),
            schema_of::<write::Moved>(output()),
        ),
        (
            schema_of::<write::RefInput>(input()),
            schema_of::<refs::Reffed>(output()),
        ),
        (
            schema_of::<write::MigrateInput>(input()),
            schema_of::<migrate::Output>(output()),
        ),
    ];
    let capabilities: Vec<Value> = (CAPABILITIES.iter().zip(schemas))
        .map(|((name, description), (input, output))| {
            let writes = !["available", "tree", "done", "move", "ref", "migrate"].contains(name);
            let description = if writes {
                format!("{description}{WRITES}")
            } else {
                (*description).to_owned()
            };
            let mut tool = json!({
                "name": name,
                "description": description,
                "inputSchema": input,
                "outputSchema": output,
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": EDITS.contains(name),
                    "idempotentHint": !["add", "done"].contains(name),
                    "openWorldHint": true,
                },
                "_meta": {},
            });
            // Every capability but `tree` syncs or pushes the task's repo,
            // which changes its clone; `tree` reads the repos at their pins.
            if *name == "tree" {
                tool["annotations"] = json!({
                    "readOnlyHint": true,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false,
                });
            }
            tool
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.5.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Checks for an open task that covers a bug or feature, then claims and \
            closes it. Use it before that work, to add a task, or when the person asks to sweep \
            or go through their tasks or to-do list.",
        "metadata": {"area": "planning"},
        "types": records::TASK_TYPES,
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;

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
pub mod records;
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

use records::TaskRecord;

/// The plugin's name, which its codes start with.
const PLUGIN: &str = "tasks";

#[derive(Clone, Copy, Debug)]
pub struct Tasks;

/// The capabilities that change an existing task. `add` writes a new
/// task, a second one on a repeat, and `done` defers a repeating task
/// again on each call.
const EDITS: [&str; 6] = ["claim", "done", "defer", "drop", "reviewed", "move"];
/// What every write call but `done` and `move` says after its own
/// description; `done` says it in its own words, since it may not push.
const WRITES: &str = " It syncs the task's repo first and pushes after, and sets reviewed.";

/// Each capability's name and description, in the order the manifest
/// lists them.
const CAPABILITIES: [(&str, &str); 9] = [
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
        "Write a new open task to tasks/<slug>.md in the repo the caller's remote routes to.",
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
];

impl Plugin for Tasks {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (TaskRecord::TYPE.parse()).expect("TYPE is a record type; a test parses it");
        vec![(
            name,
            schema_of::<TaskRecord>(SchemaSettings::draft2020_12()),
        )]
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
            other => Err(refusal(
                "tasks::call::unknown_capability",
                format!(
                    "tasks offers no capability {other}; it offers available, tree, add, claim, \
                     done, defer, drop, reviewed and move"
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
/// is none inside a repository. Both are none outside a git repository.
fn caller_origin(host: &dyn Host) -> (Option<String>, Option<String>) {
    match host.caller_remote() {
        Origin::Url(url) => (Some(url), None),
        Origin::Outside => (None, None),
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
        (schema_of::<write::AddInput>(input()), written()),
        (schema_of::<claim::ClaimInput>(input()), written()),
        (schema_of::<write::DoneInput>(input()), written()),
        (schema_of::<write::DeferInput>(input()), written()),
        (schema_of::<write::TaskPath>(input()), written()),
        (schema_of::<write::TaskPath>(input()), written()),
        (
            schema_of::<write::MoveInput>(input()),
            schema_of::<write::Moved>(output()),
        ),
    ];
    let capabilities: Vec<Value> = (CAPABILITIES.iter().zip(schemas))
        .map(|((name, description), (input, output))| {
            let writes = !["available", "tree", "done", "move"].contains(name);
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
        "version": "0.4.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Before you fix a bug or build a feature, use it to check whether an open \
            task already covers the work, and claim that task and finish it with tasks.done \
            instead of adding a new one. \
            Also use it to sweep the person's tasks one at a time, marking each done, deferring, \
            dropping or editing it, when the person asks to sweep, review or go through their \
            tasks or to-do list, and to add a task to their own list. Start with `board -C \
            <board> tasks available`, run in the repository you work in, which syncs the \
            notes repos that repository's git remote routes to and lists the tasks to sweep. \
            SKILL.md and README.md in the plugin's directory say more.",
        "metadata": {"area": "planning"},
        "types": [TaskRecord::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;

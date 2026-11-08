//! The inputs of the write calls, as their schemas describe them to a
//! caller.

use board_plugin::shape::Slug;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

use crate::records::REPEAT;

/// A date or a horizon, as `defer` and `add` take it.
const UNTIL: &str = r"^([0-9]{4}-[0-9]{2}-[0-9]{2}|not-today|not-this-week|soon|someday)$";
const UNTIL_HELP: &str = "A date, YYYY-MM-DD, or a horizon from today: not-today (1 day), \
                          not-this-week (6), soon (20) or someday (90). The task is written with \
                          the date.";

/// The task to act on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskPath {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task's path in the board, such as /tasks/call-the-bank.md or \
                       /repos/personal/tasks/x.md."
    )]
    pub(crate) path: String,
}

/// The task to mark done, and whether the call pushes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DoneInput {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task's path in the board, such as /tasks/call-the-bank.md or \
                       /repos/personal/tasks/x.md."
    )]
    pub(crate) path: String,
    #[schemars(
        description = "Whether the call pushes the task's repo, true when left out. With false, \
                       the sync before the write pulls without pushing, the task is closed in a \
                       commit in the clone alone, and `unpushed` is tasks::repo::not_pushed; a \
                       later write call pushes the commit."
    )]
    pub(crate) push: Option<bool>,
}

/// The task to defer, and until when.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeferInput {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task's path in the board, such as /tasks/call-the-bank.md or \
                       /repos/personal/tasks/x.md."
    )]
    pub(crate) path: String,
    #[schemars(regex(pattern = UNTIL), description = UNTIL_HELP)]
    pub(crate) until: String,
}

/// The task to move, and its new parent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MoveInput {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task's path in the board, such as /tasks/call-the-bank.md or \
                       /repos/personal/tasks/x.md."
    )]
    pub(crate) path: String,
    #[schemars(
        regex(pattern = r"^(none|/.*\.md)$"),
        description = "The new parent's path in the board, in the task's own repo, such as \
                       /tasks/ship-flows-editor.md, or none to clear it."
    )]
    pub(crate) parent: String,
}

/// A new open task, and the fields it starts with.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddInput {
    #[schemars(
        length(min = 1),
        description = "The task's title, which names its file once and for all."
    )]
    pub(crate) title: String,
    #[schemars(description = "The task's prose, written as its body.")]
    pub(crate) text: Option<String>,
    #[schemars(
        regex(pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"),
        description = "The date, YYYY-MM-DD, the task is due by."
    )]
    pub(crate) due: Option<String>,
    #[schemars(regex(pattern = UNTIL), description = UNTIL_HELP)]
    pub(crate) defer: Option<String>,
    #[schemars(
        description = "The paths, in the task's own repo, of the tasks it waits on, such as \
                       /tasks/agree-scope.md."
    )]
    pub(crate) after: Option<Vec<String>>,
    #[schemars(
        regex(pattern = REPEAT),
        description = "When it comes back once done: daily, weekdays, weekly-mon, monthly-15 or \
                       annually-10-25."
    )]
    pub(crate) repeat: Option<String>,
    #[schemars(description = "Whether a missed day of a repeating task is dropped.")]
    pub(crate) ephemeral: Option<bool>,
    #[schemars(
        range(min = 0, max = 23),
        description = "The hour of the person's day, 0 to 23, the task is available from."
    )]
    pub(crate) from_hour: Option<u8>,
    #[serde(default)]
    #[schemars(
        with = "Option<Vec<Slug>>",
        description = "Tags of where it can be done, such as errands."
    )]
    pub(crate) contexts: Option<Vec<String>>,
    #[serde(default)]
    #[schemars(
        schema_with = "kind",
        description = "task, as when left out, or project."
    )]
    pub(crate) kind: Option<String>,
    #[schemars(
        regex(pattern = r"^/"),
        description = "The board path, in the task's own repo, of a project's brief, such as \
                       /projects/flows/README.md."
    )]
    pub(crate) brief: Option<String>,
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The board path, in the task's own repo, of the task it is part of, such \
                       as /tasks/ship-flows-editor.md."
    )]
    pub(crate) parent: Option<String>,
    #[schemars(
        regex(pattern = r"^/"),
        description = "The repo to write the task to: / for the board's own tree, or a notes \
                       repo's mount path; the caller's git remote chooses it when left out."
    )]
    pub(crate) repo: Option<String>,
    #[schemars(
        length(min = 1),
        description = "A git remote, or owner/name, to choose the repos by in place of the origin \
                       of the directory board was started in."
    )]
    pub(crate) remote: Option<String>,
}

fn kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["task", "project"]})
}

//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed. A task
//! refuses a field the schema does not name. Reflect co-writes tasks, so the
//! schema names the `id` and `pinned` it writes.

use board_plugin::shape::{Actor, Prose, Record, Slug, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

/// The pattern of `repeat`: `daily`, `weekdays`, `weekly-<day>`,
/// `monthly-<1..31>` and `annually-<MM-DD>`.
pub(crate) const REPEAT: &str = "^(daily|weekdays|weekly-(mon|tue|wed|thu|fri|sat|sun)|monthly-([1-9]|[12][0-9]|3[01])|annually-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01]))$";

/// `tasks/task/v2`: the frontmatter of a Markdown task under `tasks/`, whose
/// body holds its prose.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Task",
    extend("dependentRequired" = {"ephemeral": ["repeat"]})
)]
pub struct TaskRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// What the task is, in a line.
    #[schemars(with = "Prose")]
    pub title: String,
    /// Whether the task is open, done or dropped.
    #[schemars(schema_with = "status")]
    pub status: String,
    /// The day the task is due.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "date")]
    pub due: Option<String>,
    /// What hides the task until it ends: a date, a date and time, or a
    /// horizon counted from `reviewed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "defer")]
    pub defer: Option<String>,
    /// The tasks in the same repo this task waits on, by path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "after")]
    pub after: Option<Vec<String>>,
    /// When a repeating task comes back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "repeat")]
    pub repeat: Option<String>,
    /// Whether a repeating task's missed days roll forward to one instance
    /// today, with no backlog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "boolean")]
    pub ephemeral: Option<bool>,
    /// The hour of the person's day from which the task is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "hour")]
    pub from_hour: Option<u8>,
    /// Tags for where the task can be done, such as `errands`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<Slug>>")]
    pub contexts: Option<Vec<String>>,
    /// The day the task was last looked at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "date")]
    pub reviewed: Option<String>,
    /// The agent or person who holds the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Actor>,
    /// The task in the same repo this one is part of, by path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "task_ref")]
    pub parent: Option<String>,
    /// Whether this is a `task`, as when it is left out, or a `project`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "kind")]
    pub kind: Option<String>,
    /// A project's brief, such as a README.md, by path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "file_ref")]
    pub brief: Option<String>,
    /// The note's id, which Reflect writes; the plugin ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub id: Option<String>,
    /// Whether, or in what place, Reflect pins the note to its sidebar,
    /// which Reflect writes; the plugin ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "pinned")]
    pub pinned: Option<Pinned>,
}

/// Reflect's `pinned`: whether the note is pinned, or its place in the
/// sidebar.
#[derive(Debug)]
pub enum Pinned {
    Pinned(bool),
    Place(u64),
}

impl Record for TaskRecord {
    const TYPE: &'static str = "tasks/task/v2";
}

fn status(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["open", "done", "dropped"]})
}

fn date(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "date"})
}

fn defer(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [
        {"type": "string", "format": "date"},
        {"type": "string", "format": "date-time"},
        {"type": "string", "enum": ["not-today", "not-this-week", "soon", "someday"]},
    ]})
}

fn task_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "board-ref", "x-board-type": TaskRecord::TYPE})
}

fn file_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "board-ref"})
}

fn after(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": task_ref(generator)})
}

fn repeat(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": REPEAT})
}

fn boolean(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "boolean"})
}

fn hour(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "integer", "minimum": 0, "maximum": 23})
}

fn pinned(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "boolean"}, {"type": "integer", "minimum": 0}]})
}

fn kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["task", "project"]})
}

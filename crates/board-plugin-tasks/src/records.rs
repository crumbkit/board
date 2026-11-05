//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed. It keeps the
//! bytes the Python plugin shipped, with a title on every property as
//! pydantic wrote it, so a board that enabled tasks before the port needs
//! no `plugin enable`. It admits fields it does not name, because Reflect
//! writes fields of its own, such as `id` and `pinned`, and a task must not
//! degrade for them.

use board_plugin::shape::{Actor, Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

/// The pattern of `repeat`: `daily`, `weekdays`, `weekly-<day>`,
/// `monthly-<1..31>` and `annually-<MM-DD>`.
pub(crate) const REPEAT: &str = "^(daily|weekdays|weekly-(mon|tue|wed|thu|fri|sat|sun)|monthly-([1-9]|[12][0-9]|3[01])|annually-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01]))$";

#[derive(Debug, JsonSchema)]
#[schemars(
    title = "Task",
    description = "`tasks/task/v2`: the frontmatter of a Markdown task under `tasks/`, whose body holds its prose.\n\n`defer` hides the task until a date, a date-time or a horizon after `reviewed`.\n`after` references the tasks in the same repo that it waits on. `repeat`\nnames when a repeating task comes back, and `ephemeral` that a missed day\nis not kept. `from_hour` is the hour of the person's day it is available\nfrom. `contexts` tags where it can be done. `reviewed` is the day it was\nlast looked at. `owner` names the agent or person who holds it, such as a\nsession id or a handle. `parent` references the task in the same repo that\nthis one is part of, and `kind` says whether it is a `task`, as when it is\nleft out, or a `project`, whose `brief` may reference a brief such as a\nREADME.md. Any other field, such as Reflect's `id` and `pinned`, is\nadmitted and ignored.",
    extend("dependentRequired" = {"ephemeral": ["repeat"]})
)]
pub struct TaskRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Title")]
    pub title: String,
    #[schemars(title = "Status", schema_with = "status")]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Due", schema_with = "date")]
    pub due: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Defer", schema_with = "defer")]
    pub defer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "After", schema_with = "after")]
    pub after: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Repeat", schema_with = "repeat")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Ephemeral", schema_with = "boolean")]
    pub ephemeral: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "From Hour", schema_with = "hour")]
    pub from_hour: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Contexts", schema_with = "contexts")]
    pub contexts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Reviewed", schema_with = "date")]
    pub reviewed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Owner")]
    pub owner: Option<Actor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Parent", schema_with = "task_ref")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Kind", schema_with = "kind")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(title = "Brief", schema_with = "file_ref")]
    pub brief: Option<String>,
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

fn contexts(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "pattern": "^[a-z0-9-]+$"}})
}

fn kind(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["task", "project"]})
}

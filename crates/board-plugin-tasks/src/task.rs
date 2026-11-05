//! A task's record as the plugin reads it: the fields of `tasks/task/v2`,
//! each checked as the Python plugin's pydantic model checked it. The
//! model refuses what the schema admits only where Python did: a date-time
//! with a leap second.

use board_repos::Date;
use serde_json::{Map, Value};

use board_plugin::shape::{Actor, Record as _};

use crate::dates::Defer;
use crate::records::TaskRecord;

/// A `tasks/task/v2` record's fields, read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) due: Option<Date>,
    /// The `defer` as written, and what it holds.
    pub(crate) defer: Option<(String, Defer)>,
    pub(crate) after: Vec<String>,
    pub(crate) from_hour: Option<u8>,
    pub(crate) contexts: Vec<String>,
    pub(crate) reviewed: Option<Date>,
    pub(crate) owner: Option<Actor>,
    pub(crate) parent: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) brief: Option<String>,
}

/// `record` read as a task, or how many of its fields the model refuses.
/// A date-time without an offset is local time, `offset` seconds east of
/// UTC.
pub(crate) fn model(record: &Value, offset: i32) -> Result<Model, usize> {
    let Some(fields) = record.as_object() else {
        return Err(1);
    };
    let mut errors = 0;
    let mut count = |ok: bool| {
        if !ok {
            errors += 1;
        }
    };
    let tag = fields.get("$type").and_then(Value::as_str);
    count(tag == Some(TaskRecord::TYPE));
    let title = fields.get("title").and_then(Value::as_str);
    count(title.is_some());
    let status = (fields.get("status").and_then(Value::as_str))
        .filter(|status| ["open", "done", "dropped"].contains(status));
    count(status.is_some());
    let due = optional(fields, "due", &mut count, |value| {
        Date::parse(value.as_str()?)
    });
    let defer = optional(fields, "defer", &mut count, |value| {
        let text = value.as_str()?;
        Some((text.to_owned(), Defer::parse(text, offset)?))
    });
    let after = list(fields, "after", &mut count, |_| true);
    optional(fields, "repeat", &mut count, |value| {
        value.as_str().filter(|rule| repeats(rule)).map(drop)
    });
    optional(fields, "ephemeral", &mut count, Value::as_bool);
    let from_hour = optional(fields, "from_hour", &mut count, |value| {
        let hour = value.as_u64().or_else(|| {
            let number = value.as_f64()?;
            (number.fract() == 0.0 && number >= 0.0).then(|| format!("{number:.0}").parse().ok())?
        })?;
        u8::try_from(hour).ok().filter(|hour| *hour <= 23)
    });
    let contexts = list(fields, "contexts", &mut count, |item| {
        !item.is_empty()
            && (item.bytes()).all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    });
    let reviewed = optional(fields, "reviewed", &mut count, |value| {
        Date::parse(value.as_str()?)
    });
    let owner = optional(fields, "owner", &mut count, |value| {
        serde_json::from_value::<Actor>(value.clone())
            .ok()
            .filter(|owner| *owner != Actor::default())
    });
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let parent = optional(fields, "parent", &mut count, text);
    let kind = optional(fields, "kind", &mut count, |value| {
        value
            .as_str()
            .filter(|kind| ["task", "project"].contains(kind))
            .map(str::to_owned)
    });
    let brief = optional(fields, "brief", &mut count, text);
    match (title, status) {
        (Some(title), Some(status)) if errors == 0 => Ok(Model {
            title: title.to_owned(),
            status: status.to_owned(),
            due,
            defer,
            after,
            from_hour,
            contexts,
            reviewed,
            owner,
            parent,
            kind,
            brief,
        }),
        _ => Err(errors.max(1)),
    }
}

/// The actor that `session`, `program` and `person` name, or none when
/// they name nobody.
pub(crate) fn actor(
    session: Option<String>,
    program: Option<String>,
    person: Option<String>,
) -> Option<Actor> {
    let actor = Actor {
        session,
        program,
        person,
    };
    (actor != Actor::default()).then_some(actor)
}

/// The field `name` read by `read`: none when it is absent or null, and
/// counted as an error when `read` refuses it.
fn optional<T>(
    fields: &Map<String, Value>,
    name: &str,
    count: &mut impl FnMut(bool),
    read: impl FnOnce(&Value) -> Option<T>,
) -> Option<T> {
    match fields.get(name) {
        None | Some(Value::Null) => None,
        Some(value) => {
            let read = read(value);
            count(read.is_some());
            read
        }
    }
}

/// The list of strings `name`, empty when it is absent, with each item
/// `keep` refuses, or a value that is no list of strings, counted.
fn list(
    fields: &Map<String, Value>,
    name: &str,
    count: &mut impl FnMut(bool),
    keep: impl Fn(&str) -> bool,
) -> Vec<String> {
    let Some(value) = fields.get(name) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        count(false);
        return Vec::new();
    };
    let mut found = Vec::new();
    for item in items {
        match item.as_str().filter(|item| keep(item)) {
            Some(item) => found.push(item.to_owned()),
            None => count(false),
        }
    }
    found
}

/// Whether `rule` is a `repeat` the schema admits: `daily`, `weekdays`,
/// `weekly-<mon..sun>`, `monthly-<1..31>` or `annually-<MM>-<DD>`.
pub(crate) fn repeats(rule: &str) -> bool {
    let number = |text: &str, two: bool, most: u8| {
        let digits = text.bytes().all(|byte| byte.is_ascii_digit());
        let shaped = if two {
            text.len() == 2
        } else {
            (1..=2).contains(&text.len()) && !text.starts_with('0')
        };
        digits
            && shaped
            && text
                .parse::<u8>()
                .is_ok_and(|value| (1..=most).contains(&value))
    };
    match rule.split_once('-') {
        None => ["daily", "weekdays"].contains(&rule),
        Some(("weekly", day)) => ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].contains(&day),
        Some(("monthly", day)) => number(day, false, 31),
        Some(("annually", date)) => date
            .split_once('-')
            .is_some_and(|(month, day)| number(month, true, 12) && number(day, true, 31)),
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests;

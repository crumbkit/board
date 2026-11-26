//! Board's filters: the list that `board search --filters` prints, the
//! short type names that `type:` takes, and the check of `type:` and
//! `path:` values.

use serde::Serialize;

use crate::error::{ErrorCode, QueryError, message};
use crate::query::{Field, Query, Term, Value};

/// What a field's values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    /// One of a fixed list of words.
    Keyword,
    /// A person, an agent or a program, or `me` for the board's owner.
    Person,
    /// The start of a logical path, such as `/tasks`.
    Path,
}

/// One of Board's filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldInfo {
    /// The field.
    pub field: Field,
    /// What its values are.
    pub kind: FieldKind,
    /// The values a builder offers.
    pub values: &'static [&'static str],
    /// What it matches, in one sentence.
    pub about: &'static str,
}

/// A short type name and the `$type` prefix it matches, such as
/// `question` and `plans/question/`, so every version of the type matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ShortType {
    /// The name `type:` takes.
    pub name: &'static str,
    /// The start of every `$type` it matches, or `None` for `doc`, which
    /// matches the entries with no `$type`.
    pub prefix: Option<&'static str>,
}

/// The short type names, in the order a builder offers them.
pub const TYPES: &[ShortType] = &[
    ShortType {
        name: "task",
        prefix: Some("tasks/task/"),
    },
    ShortType {
        name: "question",
        prefix: Some("plans/question/"),
    },
    ShortType {
        name: "answer",
        prefix: Some("plans/answer/"),
    },
    ShortType {
        name: "decision",
        prefix: Some("plans/decision/"),
    },
    ShortType {
        name: "change",
        prefix: Some("changes/change/"),
    },
    ShortType {
        name: "plan",
        prefix: Some("changes/plan/"),
    },
    ShortType {
        name: "eval",
        prefix: Some("evals/eval/"),
    },
    ShortType {
        name: "run",
        prefix: Some("runs/run/"),
    },
    ShortType {
        name: "artifact",
        prefix: Some("artifacts/artifact/"),
    },
    ShortType {
        name: "rule",
        prefix: Some("rules/rule/"),
    },
    ShortType {
        name: "doc",
        prefix: None,
    },
];

/// The values `type:` takes, as [`FIELDS`] lists them.
const TYPE_NAMES: [&str; TYPES.len()] = {
    let mut names = [""; TYPES.len()];
    let mut at = 0;
    while at < TYPES.len() {
        names[at] = TYPES[at].name;
        at += 1;
    }
    names
};

/// Board's filters, in the order a builder offers them.
pub const FIELDS: &[FieldInfo] = &[
    FieldInfo {
        field: Field::Type,
        kind: FieldKind::Keyword,
        values: &TYPE_NAMES,
        about: "The kind of record, by a short name that matches every version of its type.",
    },
    FieldInfo {
        field: Field::From,
        kind: FieldKind::Person,
        values: &["me"],
        about: "Who wrote the record: the person, agent or program its `by` names, or me for the board's owner.",
    },
    FieldInfo {
        field: Field::Path,
        kind: FieldKind::Path,
        values: &[],
        about: "The start of the entry's logical path, such as /tasks; it must start with /.",
    },
];

/// The short type that `value` names, in any case.
#[must_use]
pub fn short_type(value: &str) -> Option<ShortType> {
    TYPES
        .iter()
        .copied()
        .find(|short| short.name.eq_ignore_ascii_case(value))
}

/// The short type names joined by `, `, as an error lists them.
#[must_use]
pub fn type_names() -> String {
    TYPE_NAMES.join(", ")
}

/// The error for a `from:` value that names nobody, such as `me` when the
/// board's configuration names no owner.
#[must_use]
pub fn unknown_person(value: &Value) -> QueryError {
    message(ErrorCode::UnknownPerson, value.span, None, &value.text)
}

/// Every `type:` value of `query` that names no short type and every
/// `path:` value that does not start with `/`, in the order of the text.
#[must_use]
pub fn check(query: &Query) -> Vec<QueryError> {
    let mut errors = Vec::new();
    for part in query.parts() {
        let Term::Filter { field, values } = &part.term else {
            continue;
        };
        for value in values {
            let code = match field {
                Field::Type if short_type(&value.text).is_none() => ErrorCode::UnknownType,
                Field::Path if !value.text.starts_with('/') => ErrorCode::RelativePath,
                _ => continue,
            };
            errors.push(message(code, value.span, None, &value.text));
        }
    }
    errors
}

#[cfg(test)]
mod tests;

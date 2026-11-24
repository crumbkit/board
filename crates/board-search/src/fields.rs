//! Board's filters: the list that `board search --fields` prints, the short
//! type names that `type:` takes, and the check of `type:` values against
//! them.

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
    /// The start of every `$type` it matches.
    pub prefix: &'static str,
}

/// The short type names, in the order a builder offers them.
pub const TYPES: &[ShortType] = &[
    ShortType {
        name: "task",
        prefix: "tasks/task/",
    },
    ShortType {
        name: "question",
        prefix: "plans/question/",
    },
    ShortType {
        name: "answer",
        prefix: "plans/answer/",
    },
    ShortType {
        name: "decision",
        prefix: "plans/decision/",
    },
    ShortType {
        name: "change",
        prefix: "changes/change/",
    },
    ShortType {
        name: "plan",
        prefix: "changes/plan/",
    },
    ShortType {
        name: "eval",
        prefix: "evals/eval/",
    },
    ShortType {
        name: "run",
        prefix: "runs/run/",
    },
    ShortType {
        name: "artifact",
        prefix: "artifacts/artifact/",
    },
    ShortType {
        name: "rule",
        prefix: "rules/rule/",
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

/// Every `type:` value of `query` that names no short type, in the order
/// of the text.
#[must_use]
pub fn check(query: &Query) -> Vec<QueryError> {
    query
        .parts()
        .iter()
        .filter_map(|part| match &part.term {
            Term::Filter {
                field: Field::Type,
                values,
            } => Some(values),
            _ => None,
        })
        .flatten()
        .filter(|value| short_type(&value.text).is_none())
        .map(|value| message(ErrorCode::UnknownType, value.span, None, &value.text))
        .collect()
}

#[cfg(test)]
mod tests;

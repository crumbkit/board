//! Records and task schemas for the tests that `put` refuses with the code
//! `check` gives the same bytes (AGENTS.md rule 4). The cases and the rule
//! hold no repository, so they can also be run against a pure verdict.

use board_model::{Code, parse_json};
use proptest::prelude::*;

use crate::tests::{MANIFEST, TASK_SCHEMA};

/// Where the task schema lives in a board.
pub(crate) const TASK_SCHEMA_PATH: &str = ".board/types/lr/task/v1.schema.json";

/// What can be at the task schema's path: the task schema, nothing, or a
/// schema that cannot be used. An unreadable schema is left out: today
/// `put` says `repo::unreadable` and `check` says `record::unknown_type`, and
/// V3 of the deepening plan makes both say `repo::unreadable`.
pub(crate) fn schemas() -> Vec<Option<String>> {
    [
        Some(TASK_SCHEMA),
        None,
        Some(r#"{"type": 5}"#),
        Some(r#"{"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"}"#),
        Some("{"),
    ]
    .into_iter()
    .map(|schema| schema.map(str::to_owned))
    .collect()
}

/// Task records, each with the code `check` gives it under the task schema
/// in the test board, where `/projects/p` is a project and `/projects` a
/// directory.
pub(crate) fn tasks() -> Vec<(String, Option<Code>)> {
    [
        (
            r#", "title": "t", "project": "/projects/p", "see": "/projects/p""#,
            None,
        ),
        (
            r#", "title": "t", "project": "/projects""#,
            Some(Code::RefWrongType),
        ),
        (
            r#", "title": "t", "project": "/nowhere""#,
            Some(Code::RefDangling),
        ),
        (
            r#", "title": "t", "project": "projects/p""#,
            Some(Code::RefMalformed),
        ),
        (r#", "title": "t", "see": "x""#, Some(Code::RefMalformed)),
        ("", Some(Code::RecordInvalid)),
        (
            r#", "title": 18446744073709551616"#,
            Some(Code::JsonLossyNumber),
        ),
    ]
    .into_iter()
    .map(|(fields, code)| (format!(r#"{{"$type": "lr/task/v1"{fields}}}"#), code))
    .collect()
}

/// Records of every kind `put` and `check` judge: Board's own types, types
/// with no schema, malformed input, and the task records of [`tasks`].
pub(crate) fn records() -> Vec<String> {
    let mount = |fields: &str| format!(r#"{{"$type": "board/mount/v1"{fields}}}"#);
    let fixed = [
        r#"{"$type": "board/config/v1", "commit_on_put": false}"#,
        r#"{"$type": "board/config/v1", "colour": "red"}"#,
        MANIFEST,
        &MANIFEST.replace(r#"["./run"]"#, "[]"),
        &mount(r#", "path": "/projects/p""#),
        &mount(r#", "path": "/gone""#),
        &mount(""),
        r#"{"$type": "lr/nothing/v1"}"#,
        r#"{"$type": "board/other/v1"}"#,
        r#"{"$type": "Not a type"}"#,
        r#"{"$type": 5}"#,
        r#"{"$type": "lr/task/v1", "title": "t", "title": "u"}"#,
        "[1]",
        r#"{"title": "t"}"#,
        "",
        " \n",
        "{",
        "<<<<<<< ours\n{}\n",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    fixed
        .into_iter()
        .chain(tasks().into_iter().map(|(record, _)| record))
        .collect()
}

/// A schema slot, most often the task schema so that most generated task
/// records reach validation.
fn schema() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        3 => Just(Some(TASK_SCHEMA.to_owned())),
        2 => prop::sample::select(schemas()),
    ]
}

/// A task record with each field present or not, valid or not.
fn task() -> impl Strategy<Value = String> {
    let field = |name: &'static str, values: &'static [&'static str]| {
        prop::option::of(prop::sample::select(values))
            .prop_map(move |value| value.map(|value| format!(r#", "{name}": {value}"#)))
    };
    (
        field("title", &[r#""t""#, "5"]),
        field(
            "project",
            &[
                r#""/projects/p""#,
                r#""projects/p""#,
                r#""/nowhere""#,
                r#""/projects""#,
                r#""/.board/board""#,
            ],
        ),
        field("see", &[r#""/projects/p""#, r#""/gone""#, r#""x""#, "1"]),
        field("n", &["18446744073709551616", "1"]),
    )
        .prop_map(|(title, project, see, n)| {
            let fields: String = [title, project, see, n].into_iter().flatten().collect();
            format!(r#"{{"$type": "lr/task/v1"{fields}}}"#)
        })
}

/// One case: what is at the task schema's path, and some records.
#[derive(Clone, Debug)]
pub(crate) struct Case {
    pub(crate) schema: Option<String>,
    pub(crate) records: Vec<String>,
}

/// A schema slot and a few records, mostly generated task records.
pub(crate) fn case() -> impl Strategy<Value = Case> {
    let record = prop_oneof![
        1 => prop::sample::select(records()),
        2 => task(),
    ];
    (schema(), prop::collection::vec(record, 1..=6))
        .prop_map(|(schema, records)| Case { schema, records })
}

/// Whether `put` gives the verdict `check` gives the same bytes: it refuses
/// with `check`'s code, or admits when `check` finds nothing wrong or only a
/// reference that depends on other entries. Input that is not one JSON
/// object with a `$type` is refused as `put::invalid` before that, because
/// `check` reads it as plain JSON.
pub(crate) fn agrees(bytes: &[u8], checked: Option<Code>, put: Result<(), Code>) -> bool {
    let typed = parse_json(bytes).map_or(true, |value| {
        value
            .as_object()
            .is_some_and(|object| object.contains_key("$type"))
    });
    if !typed {
        return put == Err(Code::PutInvalid);
    }
    match checked {
        None | Some(Code::RefDangling | Code::RefWrongType) => put.is_ok(),
        Some(code) => put == Err(code),
    }
}

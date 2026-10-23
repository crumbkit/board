//! The values an error names, beside its code and message.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};

use crate::{Code, LogicalPath, ObjectId, RecordType};

/// What an error knows about the values at fault, for a failure's
/// `fields` and for filling in its hint. Each is `None` when unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    /// The logical path at fault: a mount for a mount's code or a put
    /// through a mount, a link for a dangling link.
    pub path: Option<LogicalPath>,
    /// For a path that names nothing, its deepest ancestor that resolves.
    pub ancestor: Option<LogicalPath>,
    /// The blob the path holds.
    pub blob: Option<ObjectId>,
    /// A JSON pointer into the record or input.
    pub pointer: Option<String>,
    pub record_type: Option<RecordType>,
    /// A schema's path from the board's root.
    pub schema: Option<String>,
    pub plugin: Option<String>,
    pub capability: Option<String>,
    /// A directory whose code refuses a write beneath it, by its path from
    /// the board's root, without a leading `/`.
    pub directory: Option<String>,
    /// The names a query could have used instead of one that is missing.
    pub known: Option<Known>,
    /// What a put that `--expect` refused found at its path.
    pub found: Option<Found>,
    /// Each line of a batch that refused it, in order.
    pub refusals: Vec<Lined>,
    /// The text of a symlink: the path it names.
    pub target: Option<String>,
}

/// What a put found where it would write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    Nothing,
    Blob(ObjectId),
}

/// One line of a batch, counted from 1, and why it is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lined {
    pub line: usize,
    pub code: Code,
    pub message: String,
    pub fields: Fields,
}

impl Lined {
    pub fn new(line: usize, code: Code, message: impl fmt::Display, fields: Fields) -> Self {
        Self {
            line,
            code,
            message: message.to_string(),
            fields,
        }
    }
}

/// The names a statement could have used instead of one that is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Known {
    /// Every table and view, and each attached plugin database.
    Tables(Vec<String>),
    /// Each table or view the statement names, with its columns; empty when
    /// it names none.
    Columns(BTreeMap<String, Vec<String>>),
}

impl Fields {
    /// Fields naming only `path`.
    pub fn at(path: &LogicalPath) -> Self {
        Self {
            path: Some(path.clone()),
            ..Self::default()
        }
    }

    /// The values that are known, by their names in a failure's `fields`.
    pub fn json(&self) -> Map<String, Value> {
        let strings = [
            ("path", self.path.as_ref().map(ToString::to_string)),
            ("ancestor", self.ancestor.as_ref().map(ToString::to_string)),
            ("blob", self.blob.as_ref().map(ToString::to_string)),
            ("pointer", self.pointer.clone()),
            ("type", self.record_type.as_ref().map(ToString::to_string)),
            ("schema", self.schema.clone()),
            ("plugin", self.plugin.clone()),
            ("capability", self.capability.clone()),
            ("directory", self.directory.clone()),
            ("target", self.target.clone()),
        ];
        let mut json: Map<String, Value> = strings
            .into_iter()
            .filter_map(|(name, value)| Some((name.to_owned(), Value::String(value?))))
            .collect();
        match &self.known {
            Some(Known::Tables(tables)) => {
                json.insert("tables".to_owned(), tables.clone().into());
            }
            Some(Known::Columns(columns)) => {
                let columns = columns
                    .iter()
                    .map(|(table, names)| (table.clone(), names.clone().into()));
                json.insert("columns".to_owned(), Value::Object(columns.collect()));
            }
            None => {}
        }
        if let Some(found) = self.found {
            let found = match found {
                Found::Nothing => Value::Null,
                Found::Blob(blob) => blob.to_string().into(),
            };
            json.insert("found".to_owned(), found);
        }
        if !self.refusals.is_empty() {
            let refusals = self.refusals.iter().map(|lined| {
                let (code, message) = (lined.code.as_str(), &lined.message);
                let mut entry = json!({ "line": lined.line, "code": code, "message": message });
                let fields = lined.fields.json();
                if !fields.is_empty() {
                    entry["fields"] = fields.into();
                }
                entry
            });
            json.insert("refusals".to_owned(), refusals.collect());
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;

    #[test]
    fn fields_at_a_path_name_that_path_and_nothing_else() {
        let path: LogicalPath = "/tasks/a".parse().unwrap();
        let at = Fields::at(&path);
        assert_eq!(at.path, Some(path));
        assert_eq!(Fields { path: None, ..at }, Fields::default());
    }

    #[test]
    fn fields_name_only_the_values_that_are_known() {
        assert_eq!(Fields::default().json(), Map::new());
        let fields = Fields {
            pointer: Some("/title".to_owned()),
            plugin: Some("tasks".to_owned()),
            directory: Some("tasks".to_owned()),
            ..Fields::default()
        };
        assert_eq!(
            Value::Object(fields.json()),
            json!({ "pointer": "/title", "plugin": "tasks", "directory": "tasks" })
        );
    }

    #[test]
    fn fields_name_the_tables_or_each_tables_columns_a_query_could_have_used() {
        let tables = Fields {
            known: Some(Known::Tables(vec![
                "entries".to_owned(),
                "records".to_owned(),
            ])),
            ..Fields::default()
        };
        assert_eq!(
            Value::Object(tables.json()),
            json!({ "tables": ["entries", "records"] })
        );
        let columns = Fields {
            known: Some(Known::Columns(BTreeMap::from([
                ("blobs".to_owned(), vec!["blob".to_owned()]),
                (
                    "entries".to_owned(),
                    vec!["path".to_owned(), "logical".to_owned()],
                ),
            ]))),
            ..Fields::default()
        };
        assert_eq!(
            Value::Object(columns.json()),
            json!({ "columns": { "blobs": ["blob"], "entries": ["path", "logical"] } })
        );
        let none = Fields {
            known: Some(Known::Columns(BTreeMap::new())),
            ..Fields::default()
        };
        assert_eq!(Value::Object(none.json()), json!({ "columns": {} }));
    }

    #[test]
    fn fields_name_what_a_stale_put_found_and_each_line_a_batch_refused() {
        let blob = ObjectId::Sha1([1; 20]);
        let found = |found| {
            Value::Object(
                Fields {
                    found: Some(found),
                    ..Fields::default()
                }
                .json(),
            )
        };
        assert_eq!(found(Found::Nothing), json!({ "found": null }));
        assert_eq!(
            found(Found::Blob(blob)),
            json!({ "found": blob.to_string() })
        );
        let path: LogicalPath = "/tasks/a".parse().unwrap();
        let lined = |line, fields| Lined::new(line, Code::PutInvalid, "it has no $type", fields);
        let batch = Fields {
            refusals: vec![lined(2, Fields::at(&path)), lined(5, Fields::default())],
            ..Fields::default()
        };
        assert_eq!(
            Value::Object(batch.json()),
            json!({ "refusals": [
                { "line": 2, "code": "board::put::invalid", "message": "it has no $type", "fields": { "path": "/tasks/a" } },
                { "line": 5, "code": "board::put::invalid", "message": "it has no $type" },
            ] })
        );
    }

    /// The keys whose rows open the table under `## What an error names`.
    fn documented(doc: &str) -> BTreeSet<String> {
        let table = doc
            .split("## What an error names")
            .nth(1)
            .unwrap_or_default();
        let keys = table.lines().filter_map(|line| line.strip_prefix("| `"));
        keys.filter_map(|key| Some(key.split('`').next()?.to_owned()))
            .collect()
    }

    /// Every key `json` emits, with every field set, and the keys a call's
    /// refusal adds beside them.
    fn emitted() -> BTreeSet<String> {
        let tables = Fields {
            path: Some("/tasks/a".parse().unwrap()),
            ancestor: Some("/tasks".parse().unwrap()),
            blob: Some(ObjectId::Sha1([1; 20])),
            pointer: Some("/title".to_owned()),
            record_type: Some("lr/note/v1".parse().unwrap()),
            schema: Some(".board/types/lr/note/v1.schema.json".to_owned()),
            plugin: Some("tasks".to_owned()),
            capability: Some("due".to_owned()),
            directory: Some("tasks".to_owned()),
            known: Some(Known::Tables(Vec::new())),
            found: Some(Found::Nothing),
            target: Some("../x.json".to_owned()),
            refusals: vec![Lined::new(1, Code::PutInvalid, "no", Fields::default())],
        };
        let columns = Fields {
            known: Some(Known::Columns(BTreeMap::new())),
            ..tables.clone()
        };
        let call = ["offered", "nearest", "plugins", "stderr"].map(str::to_owned);
        let keys = |fields: &Fields| fields.json().into_iter().map(|(key, _)| key);
        keys(&tables).chain(keys(&columns)).chain(call).collect()
    }

    const ERRORS_DOC: &str = include_str!("../../../docs/architecture/errors.md");

    #[test]
    fn the_errors_doc_names_every_key_an_error_can_carry_in_fields_and_no_other() {
        assert_eq!(documented(ERRORS_DOC), emitted());
    }

    #[test]
    fn an_errors_doc_that_misses_a_key_or_names_another_does_not_match() {
        let twin = ERRORS_DOC
            .replace("| `found` |", "| found |")
            .replace("| `path` |", "| `ghost` |\n| `path` |");
        let mut expected = emitted();
        expected.remove("found");
        expected.insert("ghost".to_owned());
        assert_eq!(documented(&twin), expected);
    }
}

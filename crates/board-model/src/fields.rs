//! The values an error names, beside its code and message.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{LogicalPath, ObjectId, RecordType};

/// What an error knows about the values at fault, for `board/error/v1`
/// `fields` and for filling in its hint. Each is `None` when unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    /// The logical path at fault: a mount for a mount's code, a link for a
    /// dangling link.
    pub path: Option<LogicalPath>,
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

    /// The values that are known, by their `board/error/v1` names.
    pub fn json(&self) -> Map<String, Value> {
        let strings = [
            ("path", self.path.as_ref().map(ToString::to_string)),
            ("blob", self.blob.as_ref().map(ToString::to_string)),
            ("pointer", self.pointer.clone()),
            ("type", self.record_type.as_ref().map(ToString::to_string)),
            ("schema", self.schema.clone()),
            ("plugin", self.plugin.clone()),
            ("capability", self.capability.clone()),
            ("directory", self.directory.clone()),
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
        json
    }
}

#[cfg(test)]
mod tests {
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
}

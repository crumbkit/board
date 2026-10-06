//! The values an error names, beside its code and message.

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
        let known = [
            ("path", self.path.as_ref().map(ToString::to_string)),
            ("blob", self.blob.as_ref().map(ToString::to_string)),
            ("pointer", self.pointer.clone()),
            ("type", self.record_type.as_ref().map(ToString::to_string)),
            ("schema", self.schema.clone()),
            ("plugin", self.plugin.clone()),
            ("capability", self.capability.clone()),
        ];
        known
            .into_iter()
            .filter_map(|(name, value)| Some((name.to_owned(), Value::String(value?))))
            .collect()
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
            ..Fields::default()
        };
        assert_eq!(
            Value::Object(fields.json()),
            json!({ "pointer": "/title", "plugin": "tasks" })
        );
    }
}

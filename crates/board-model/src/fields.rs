//! The values an error names, beside its code and message.

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_at_a_path_name_that_path_and_nothing_else() {
        let path: LogicalPath = "/tasks/a".parse().unwrap();
        let at = Fields::at(&path);
        assert_eq!(at.path, Some(path));
        assert_eq!(Fields { path: None, ..at }, Fields::default());
    }
}

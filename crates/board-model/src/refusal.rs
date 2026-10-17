//! A refused command, as every interface reports it (STANDARDS.md 25).

use std::fmt;

use serde_json::{Map, Value};

use crate::{Code, Fields, Lined};

/// The code a refusal carries: one of Board's, or a plugin's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefusalCode {
    Board(Code),
    /// A plugin's code, `<plugin>::<area>::<reason>`, as the plugin gave it.
    Plugin(String),
}

impl RefusalCode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Board(code) => code.as_str(),
            Self::Plugin(code) => code,
        }
    }
}

/// What failed, for which values, and what to try next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: RefusalCode,
    /// What was wrong, naming the value at fault.
    pub message: String,
    /// What to try next. A plugin's own code has none from Board.
    pub hint: Option<String>,
    /// The values at fault, by their names in a failure's `fields`.
    pub fields: Map<String, Value>,
    /// The end of a plugin's stderr, or empty.
    pub stderr: Vec<u8>,
    /// Each line of a batch that refused it, in order.
    pub lines: Vec<Lined>,
}

impl Refusal {
    /// Board's refusal with `code`, naming the values in `fields`, with the
    /// code's hint filled in from them.
    pub fn new(code: Code, message: impl fmt::Display, fields: &Fields) -> Self {
        Self {
            code: RefusalCode::Board(code),
            message: message.to_string(),
            hint: Some(code.hint(fields)),
            fields: fields.json(),
            stderr: Vec::new(),
            lines: fields.refusals.clone(),
        }
    }

    /// This refusal with `hint` in place of its code's hint, for a caller
    /// that knows more than the code.
    #[must_use]
    pub fn with_hint(self, hint: impl Into<String>) -> Self {
        Self {
            hint: Some(hint.into()),
            ..self
        }
    }

    /// A plugin's refusal with its own `code` and `fields`, and no hint.
    pub fn plugin(code: String, message: String, fields: Map<String, Value>) -> Self {
        Self {
            code: RefusalCode::Plugin(code),
            message,
            hint: None,
            fields,
            stderr: Vec::new(),
            lines: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::{Found, Known, Lined, ObjectId};

    fn generic(code: Code) -> String {
        code.hint(&Fields::default())
    }

    #[test]
    fn a_refusal_with_a_board_code_carries_that_codes_hint_filled_from_its_fields() {
        let fields = Fields::at(&"/tasks/t9".parse().unwrap());
        for code in Code::ALL {
            let refusal = Refusal::new(code, "nothing is at /tasks/t9", &fields);
            assert_eq!(refusal.code.as_str(), code.as_str());
            assert_eq!(refusal.hint, Some(code.hint(&fields)));
            assert_eq!(refusal.fields, fields.json());
        }
        let missing = Fields {
            ancestor: Some("/tasks".parse().unwrap()),
            ..fields
        };
        let refusal = Refusal::new(Code::PathNotFound, "nothing is at /tasks/t9", &missing);
        assert_eq!(
            refusal.hint.as_deref(),
            Some("run `board ls /tasks` to see what is there; paths leave out `.json`")
        );
        assert_eq!(
            Value::Object(refusal.fields),
            json!({ "path": "/tasks/t9", "ancestor": "/tasks" })
        );
    }

    #[test]
    fn a_path_refusal_that_names_a_directory_has_a_hint_about_that_directory() {
        let beneath = Fields {
            directory: Some("Projects".to_owned()),
            ..Fields::at(&"/Projects/q".parse().unwrap())
        };
        assert_eq!(
            Code::PathCaseCollision.hint(&beneath),
            "move any files in `Projects` beneath the spelling the message names, then write there"
        );
        assert_eq!(
            Code::PathMalformed.hint(&beneath),
            "the logical path beneath `Projects` would be over 4096 bytes; write to a path with fewer or shorter segments, or link to a shallower target"
        );
    }

    #[test]
    fn a_merge_conflict_at_a_path_hints_checking_that_path_and_one_at_none_hints_a_placeholder() {
        assert_eq!(
            Code::JsonMergeConflict.hint(&Fields::at(&"/tasks/t".parse().unwrap())),
            "resolve the git merge conflict in the file, then run `board check /tasks/t`"
        );
        assert_eq!(
            Code::JsonMergeConflict.hint(&Fields::default()),
            "resolve the git merge conflict in the file, then run `board check <path>`"
        );
    }

    #[test]
    fn a_path_refusal_that_names_no_directory_has_the_generic_hint() {
        let at = Fields::at(&"/Projects/q".parse().unwrap());
        assert_eq!(
            Code::PathCaseCollision.hint(&at),
            "rename one of the names that differ only in case; a macOS working tree cannot hold both"
        );
        assert_eq!(
            Code::PathMalformed.hint(&at),
            "a logical path starts with `/`, leaves out `.json`, and has no empty, `.` or `..` segment"
        );
    }

    #[test]
    fn a_put_through_a_named_mount_hints_updating_it_and_one_naming_none_has_the_generic_hint() {
        assert_eq!(
            Code::PutThroughMount.hint(&Fields::at(&"/vendor/m".parse().unwrap())),
            "write in the mounted repository and run `board mount update /vendor/m`; a put writes through one mount whose board commits its puts and whose HEAD is on the branch the mount follows"
        );
        assert_eq!(
            Code::PutThroughMount.hint(&Fields::default()),
            "write in the mounted repository and run `board mount update <path>`; a put writes through one mount whose board commits its puts and whose HEAD is on the branch the mount follows"
        );
    }

    #[test]
    fn a_refusal_that_names_a_schema_hints_at_that_file_and_one_without_has_the_generic_hint() {
        let schema = Fields {
            schema: Some(".board/types/lr/note/v1.schema.json".to_owned()),
            ..Fields::at(&"/notes/a".parse().unwrap())
        };
        assert_eq!(
            Code::RepoUnreadable.hint(&schema),
            "check that `.board/types/lr/note/v1.schema.json` can be read, then run the command again"
        );
        assert_eq!(
            Code::RepoUnreadable.hint(&Fields::at(&"/notes/a".parse().unwrap())),
            "check that git can read the repository, for example with `git fsck`"
        );
        assert_eq!(
            Code::SchemaInvalid.hint(&schema),
            "fix the schema at `.board/types/lr/note/v1.schema.json` so it compiles as JSON Schema 2020-12; `board check /.board/types` says what failed"
        );
        assert_eq!(
            Code::SchemaInvalid.hint(&Fields::at(&"/notes/a".parse().unwrap())),
            "fix the schema so it compiles as JSON Schema 2020-12; the message says what failed"
        );
    }

    #[test]
    fn a_stale_refusal_hints_the_expect_that_matches_what_it_found() {
        let blob = ObjectId::Sha1([1; 20]);
        let stale = |found| Fields {
            found: Some(found),
            ..Fields::at(&"/tasks/a".parse().unwrap())
        };
        assert_eq!(
            Code::PutStale.hint(&stale(Found::Blob(blob))),
            format!(
                "read the record again with `board get /tasks/a`, reapply your change, and put it with `board put /tasks/a --expect {blob}`"
            )
        );
        assert_eq!(
            Code::PutStale.hint(&stale(Found::Nothing)),
            "nothing is at /tasks/a; put it as a new file with `board put /tasks/a --expect none`"
        );
        assert_eq!(
            Code::PutStale.hint(&Fields::at(&"/tasks/a".parse().unwrap())),
            Code::PutStale.hint(&Fields::default())
        );
    }

    #[test]
    fn a_put_refusal_that_lists_refused_lines_has_the_batch_hint_and_one_without_has_its_own() {
        let line = Lined::new(2, Code::PutStale, "it holds nothing", Fields::default());
        let batch = Fields {
            refusals: vec![line],
            ..Fields::default()
        };
        let refused = Refusal::new(Code::PutStale, "the batch is refused", &batch);
        assert_eq!(refused.lines, batch.refusals);
        for code in [Code::PutInvalid, Code::PutStale] {
            assert_eq!(
                code.hint(&batch),
                "fix each line listed, then send the whole batch again; nothing was written"
            );
            assert_eq!(code.hint(&Fields::default()), generic(code));
        }
        assert_eq!(Code::JsonParse.hint(&batch), generic(Code::JsonParse));
    }

    #[test]
    fn a_query_refusal_that_names_the_names_there_are_has_a_hint_that_lists_them() {
        let known = |known: Known| Fields {
            known: Some(known),
            ..Fields::default()
        };
        let tables = known(Known::Tables(vec![
            "entries".to_owned(),
            "records".to_owned(),
        ]));
        assert_eq!(
            Code::QueryInvalid.hint(&tables),
            "the tables and views are entries, records; `board query --help` describes them"
        );
        let columns = known(Known::Columns(BTreeMap::from([
            ("blobs".to_owned(), vec!["blob".to_owned()]),
            (
                "entries".to_owned(),
                vec!["path".to_owned(), "logical".to_owned()],
            ),
        ])));
        assert_eq!(
            Code::QueryInvalid.hint(&columns),
            "the columns are blobs: blob; entries: path, logical"
        );
        assert_eq!(
            Code::QueryInvalid.hint(&known(Known::Columns(BTreeMap::new()))),
            "run `board query \"SELECT name, sql FROM sqlite_master\"` for every table and its columns"
        );
        let refusal = Refusal::new(Code::QueryInvalid, "no such table: entires", &tables);
        assert_eq!(refusal.lines, []);
        assert_eq!(refusal.hint, Some(Code::QueryInvalid.hint(&tables)));
        assert_eq!(refusal.fields, tables.json());
    }

    #[test]
    fn a_query_refusal_that_names_no_names_has_the_generic_hint() {
        assert_eq!(
            Code::QueryInvalid.hint(&Fields::default()),
            "fix the statement as the message describes; Board runs one read-only SQL statement, and `board query --help` lists the tables"
        );
    }

    #[test]
    fn a_refusal_given_a_hint_carries_it_in_place_of_its_codes_hint_and_keeps_the_rest() {
        let fields = Fields::at(&"/tasks/t9.json".parse().unwrap());
        let refusal = Refusal::new(Code::PathNotFound, "nothing is at /tasks/t9.json", &fields);
        let hinted = refusal.clone().with_hint("try `board ls /tasks/t9`");
        assert_eq!(hinted.hint.as_deref(), Some("try `board ls /tasks/t9`"));
        assert_eq!(
            Refusal {
                hint: refusal.hint.clone(),
                ..hinted
            },
            refusal
        );
    }

    #[test]
    fn a_refusal_with_a_plugin_code_has_no_hint_and_keeps_the_plugins_code_and_fields() {
        let fields = Map::from_iter([
            ("due".to_owned(), json!("2026-10-06")),
            ("refusals".to_owned(), json!([{"line": 1}])),
        ]);
        let refusal = Refusal::plugin(
            "tasks::due::past".to_owned(),
            "the date is past".to_owned(),
            fields.clone(),
        );
        assert_eq!(refusal.code.as_str(), "tasks::due::past");
        assert_eq!(refusal.hint, None);
        assert_eq!(refusal.fields, fields);
        assert_eq!(refusal.lines, []);
    }
}

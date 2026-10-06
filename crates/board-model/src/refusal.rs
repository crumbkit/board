//! A refused command, as every interface reports it (STANDARDS.md 25).

use std::fmt;

use serde_json::{Map, Value};

use crate::{Code, Fields};

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
    /// The values at fault, by their `board/error/v1` `fields` names.
    pub fields: Map<String, Value>,
    /// The end of a plugin's stderr, or empty.
    pub stderr: Vec<u8>,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_refusal_with_a_board_code_carries_that_codes_hint_filled_from_its_fields() {
        let fields = Fields::at(&"/tasks/t9".parse().unwrap());
        for code in Code::ALL {
            let refusal = Refusal::new(code, "nothing is at /tasks/t9", &fields);
            assert_eq!(refusal.code.as_str(), code.as_str());
            assert_eq!(refusal.hint, Some(code.hint(&fields)));
            assert_eq!(refusal.fields, fields.json());
        }
        let refusal = Refusal::new(Code::PathNotFound, "nothing is at /tasks/t9", &fields);
        assert_eq!(
            refusal.hint.as_deref(),
            Some("run `board ls /tasks` to see what is there; paths leave out `.json`")
        );
        assert_eq!(
            Value::Object(refusal.fields),
            json!({ "path": "/tasks/t9" })
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
        let fields = Map::from_iter([("due".to_owned(), json!("2026-10-06"))]);
        let refusal = Refusal::plugin(
            "tasks::due::past".to_owned(),
            "the date is past".to_owned(),
            fields.clone(),
        );
        assert_eq!(refusal.code.as_str(), "tasks::due::past");
        assert_eq!(refusal.hint, None);
        assert_eq!(refusal.fields, fields);
    }
}

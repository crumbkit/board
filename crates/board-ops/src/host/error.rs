//! A built-in plugin's own refusal.

use board_model::Refusal;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// A built-in plugin's own refusal, whose code is
/// `<plugin>::<area>::<reason>` with the plugin's own name, as in
/// `board/error/v1`.
#[derive(Clone, Debug)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    pub fields: Map<String, Value>,
    /// The next thing to try, which Board prints after the message, as a
    /// process plugin's `hint`.
    pub hint: Option<String>,
}

impl PluginError {
    /// `<plugin>::board::refused`, as a process plugin's Python helper
    /// reports it. Its hint is the hint of Board's refusal. When that
    /// refusal has none, the hint says to fix what Board's code names and
    /// to run the command alone to see Board's own refusal.
    pub fn board_refused(plugin: &str, command: &str, refusal: &Refusal) -> Self {
        let code = refusal.code.as_str();
        let hint = (refusal.hint.clone().filter(|hint| !hint.is_empty())).unwrap_or_else(|| {
            format!(
                "fix what {code} names, then run `board {command}` on its own to see Board's \
                 refusal and its help"
            )
        });
        let mut fields = refusal.fields.clone();
        fields.insert("code".to_owned(), Value::from(code));
        Self {
            code: format!("{plugin}::board::refused"),
            message: format!("`board {command}` refused with {code}: {}", refusal.message),
            fields,
            hint: Some(hint),
        }
    }

    /// Reads `input`, which passed `capability`'s input schema, as `T`, or
    /// refuses with `<plugin>::call::bad_input`, since then `T` and the
    /// schema disagree.
    pub fn read_input<T: DeserializeOwned>(
        plugin: &str,
        capability: &str,
        input: &Value,
    ) -> Result<T, Self> {
        T::deserialize(input).map_err(|error| Self {
            code: format!("{plugin}::call::bad_input"),
            message: format!(
                "{capability}'s input {input} passed its schema but does not read: {error}; \
                 the input struct and its schema disagree, so report this to Board's developers"
            ),
            fields: Map::new(),
            hint: None,
        })
    }
}

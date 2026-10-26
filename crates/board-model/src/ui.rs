//! Board's UI records, `ui/*`, and the catalogue of component kinds the app
//! draws (docs/architecture/ui/views.md). Like `board/*`, the `ui` namespace is
//! Board's own, so these records are read by their Rust types.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::code::Code;
use crate::path::LogicalPath;

mod patch;

use patch::Patch;
pub use patch::PatchError;

/// The `$type` of a view: named slots, each with a component kind.
pub const VIEW_TYPE: &str = "ui/view/v1";
/// The `$type` of a binding: a view, and what fills each of its slots.
pub const BINDING_TYPE: &str = "ui/binding/v1";
/// The `$type` of content derived for a page, such as a written summary.
pub const DERIVED_TYPE: &str = "ui/derived/v1";

/// A component kind the app draws, with the JSON Schema a slot's data or
/// referenced record must satisfy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    Markdown,
    Record,
    Diff,
}

impl ComponentKind {
    pub const ALL: [Self; 3] = [Self::Markdown, Self::Record, Self::Diff];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Record => "record",
            Self::Diff => "diff",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Markdown => {
                "GitHub-flavoured Markdown with highlighted code, heading anchors, a contents rail, footnotes and task lists"
            }
            Self::Record => {
                "any record as a definition list, with nested values folded and references as links"
            }
            Self::Diff => {
                "a unified diff, given as data, as its files with change counts and numbered lines that link to themselves"
            }
        }
    }

    /// The JSON Schema that a slot's data or referenced record must satisfy.
    pub fn schema(self) -> Value {
        match self {
            Self::Markdown => json!({
                "type": "object",
                "required": ["markdown"],
                "properties": {"markdown": {"type": "string"}},
            }),
            Self::Record => json!({"type": "object"}),
            Self::Diff => json!({
                "type": "object",
                "required": ["patch"],
                "properties": {"patch": {"type": "string"}},
            }),
        }
    }

    /// The record types a slot of this kind draws by reference; `*` is any.
    pub const fn draws(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &[DERIVED_TYPE],
            Self::Record => &["*"],
            Self::Diff => &[],
        }
    }

    /// A slot's fill, as JSON text, to edit into a binding.
    pub const fn template(self) -> &'static str {
        match self {
            Self::Markdown => {
                r##"{"data": {"markdown": "# Summary\n\nWhat changed, and why.\n\n- [x] Tests pass\n- [ ] Owner reviewed\n\n```rust\nfn main() {}\n```\n"}}"##
            }
            Self::Record => r#"{"ref": "/tasks/a"}"#,
            Self::Diff => {
                r#"{"data": {"patch": "diff --git a/src/retry.rs b/src/retry.rs\n--- a/src/retry.rs\n+++ b/src/retry.rs\n@@ -1,3 +1,3 @@\n fn limit() -> u32 {\n-    3\n+    5\n }\n"}}"#
            }
        }
    }

    /// What the app draws for a value that satisfies [`Self::schema`]: the
    /// value itself, or for a diff, its patch read into files.
    ///
    /// # Panics
    ///
    /// Never: a patch holds only strings, numbers and lists, which always
    /// serialize.
    pub fn drawn(self, value: Value) -> Result<Value, PatchError> {
        match self {
            Self::Markdown | Self::Record => Ok(value),
            Self::Diff => {
                let text = value.get("patch").and_then(Value::as_str).unwrap_or("");
                let patch = Patch::parse(text)?;
                Ok(serde_json::to_value(patch)
                    .expect("a patch serializes: it holds only strings, numbers and lists"))
            }
        }
    }
}

/// Why a `ui/*` record does not read as its type.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("it is not a {record_type} record: {source}")]
    Shape {
        record_type: &'static str,
        source: serde_json::Error,
    },
    #[error("slot {slot:?} names kind {kind:?}, which is not in the catalogue")]
    UnknownKind {
        at: usize,
        slot: String,
        kind: String,
    },
    #[error("slot {slot:?} is named more than once")]
    Duplicate { at: usize, slot: String },
    #[error("it names no record in `from`")]
    NoSource,
}

impl UiError {
    pub fn code(&self) -> Code {
        match self {
            Self::UnknownKind { .. } => Code::UiUnknownKind,
            Self::Shape { .. } | Self::Duplicate { .. } | Self::NoSource => Code::RecordInvalid,
        }
    }

    /// The JSON pointer at fault, when Board knows it.
    pub fn pointer(&self) -> Option<String> {
        match self {
            Self::UnknownKind { at, .. } => Some(format!("/slots/{at}/kind")),
            Self::Duplicate { at, .. } => Some(format!("/slots/{at}/name")),
            Self::NoSource => Some("/from".to_owned()),
            Self::Shape { .. } => None,
        }
    }
}

/// A view: its slots in the order the page shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub name: String,
    pub kind: ComponentKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawView {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    slots: Vec<RawSlot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSlot {
    name: String,
    kind: String,
}

impl View {
    /// The view a parsed record of type [`VIEW_TYPE`] holds.
    pub fn from_value(value: &Value) -> Result<Self, UiError> {
        let raw = RawView::deserialize(value).map_err(|source| UiError::Shape {
            record_type: VIEW_TYPE,
            source,
        })?;
        let mut names = BTreeSet::new();
        let mut slots = Vec::with_capacity(raw.slots.len());
        for (at, RawSlot { name, kind }) in raw.slots.into_iter().enumerate() {
            let Some(parsed) = ComponentKind::parse(&kind) else {
                return Err(UiError::UnknownKind {
                    at,
                    slot: name,
                    kind,
                });
            };
            if !names.insert(name.clone()) {
                return Err(UiError::Duplicate { at, slot: name });
            }
            slots.push(Slot { name, kind: parsed });
        }
        Ok(Self { slots })
    }
}

/// What fills one slot: a record by reference, or data given inline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Fill {
    Ref(LogicalPath),
    Data(Value),
}

/// A binding: the view it fills, and the fill of each slot by name.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub view: LogicalPath,
    pub slots: BTreeMap<String, Fill>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinding {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    view: LogicalPath,
    slots: BTreeMap<String, Fill>,
}

impl Binding {
    /// The binding a parsed record of type [`BINDING_TYPE`] holds.
    pub fn from_value(value: &Value) -> Result<Self, UiError> {
        let raw = RawBinding::deserialize(value).map_err(|source| UiError::Shape {
            record_type: BINDING_TYPE,
            source,
        })?;
        Ok(Self {
            view: raw.view,
            slots: raw.slots,
        })
    }
}

/// Content derived for a page from other records, and who wrote it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDerived {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    from: Vec<LogicalPath>,
    #[serde(rename = "agent")]
    _agent: String,
    #[serde(rename = "model")]
    _model: String,
    #[serde(rename = "markdown")]
    _markdown: String,
}

/// Checks a parsed record of type [`DERIVED_TYPE`]: it names the records it
/// came from, the agent and model that wrote it, and its Markdown.
pub(crate) fn check_derived(value: &Value) -> Result<(), UiError> {
    let raw = RawDerived::deserialize(value).map_err(|source| UiError::Shape {
        record_type: DERIVED_TYPE,
        source,
    })?;
    if raw.from.is_empty() {
        return Err(UiError::NoSource);
    }
    Ok(())
}

/// Checks a parsed record of one of the `ui/*` types Board knows.
pub fn check(record_type: &str, value: &Value) -> Option<Result<(), UiError>> {
    Some(match record_type {
        VIEW_TYPE => View::from_value(value).map(drop),
        BINDING_TYPE => Binding::from_value(value).map(drop),
        DERIVED_TYPE => check_derived(value),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

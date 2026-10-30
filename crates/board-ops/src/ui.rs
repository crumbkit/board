//! Pages that `board serve` draws: a binding fitted to its view, by one rule
//! that `put` and `check` share, and a binding with every slot read for the
//! app (docs/architecture/ui/views.md).

use std::collections::BTreeSet;

use board_model::{
    BINDING_TYPE, Binding, Classified, Code, ComponentKind, Content, Fields, Fill, Location,
    LogicalPath, Object, ObjectId, Refusal, UiError, VIEW_TYPE, View, classify,
};
use board_tree::{Contents, GitError, ResolveError, Resolver};
use board_validate::Schema;
use serde::Serialize;
use serde_json::Value;

use crate::board::Board;
use crate::check::Detail;
use crate::read::{ReadError, read_at};

/// Reads the record at a path, or gives the code and reason it cannot.
type Read<'a> = dyn FnMut(&LogicalPath) -> Result<Value, (Code, String)> + 'a;

/// Why a binding does not fit its view.
#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    /// The view, or a record a slot references, is missing, is not a
    /// record, or is of a type the slot does not draw.
    #[error("{target}, at {pointer}, {reason}")]
    Target {
        pointer: String,
        target: LogicalPath,
        code: Code,
        reason: String,
    },
    #[error("its view is not a ui/view/v1 record: {0}")]
    View(UiError),
    #[error("it fills slots {found:?}, but its view {view} names slots {expected:?}")]
    Mismatch {
        view: LogicalPath,
        expected: Vec<String>,
        found: Vec<String>,
    },
    #[error("slot {slot:?} does not satisfy the {kind} kind: {reason}")]
    Invalid {
        pointer: String,
        slot: String,
        kind: &'static str,
        reason: String,
    },
}

impl BindingError {
    pub fn code(&self) -> Code {
        match self {
            Self::Target { code, .. } => *code,
            Self::View(error) => error.code(),
            Self::Mismatch { .. } => Code::UiSlotMismatch,
            Self::Invalid { .. } => Code::UiSlotInvalid,
        }
    }

    pub fn fields(&self) -> Fields {
        let (pointer, target) = match self {
            Self::Target {
                pointer, target, ..
            } => (pointer.clone(), Some(target.clone())),
            Self::View(_) => ("/view".to_owned(), None),
            Self::Mismatch { view, .. } => ("/slots".to_owned(), Some(view.clone())),
            Self::Invalid { pointer, .. } => (pointer.clone(), None),
        };
        Fields {
            pointer: Some(pointer),
            target: target.map(|target| target.to_string()),
            ..Fields::default()
        }
    }
}

/// One slot as the app draws it: its kind, and its data or the record it
/// references, or why it cannot be drawn.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Drawn {
    pub name: String,
    pub kind: &'static str,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<LogicalPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<Failed>,
}

/// Why a slot cannot be drawn, as the app shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Failed {
    pub code: &'static str,
    pub message: String,
    pub hint: String,
}

impl From<&BindingError> for Failed {
    fn from(error: &BindingError) -> Self {
        Self {
            code: error.code().as_str(),
            message: error.to_string(),
            hint: error.code().hint(&error.fields()),
        }
    }
}

/// A binding with each of its view's slots read, in the view's order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page {
    pub binding: LogicalPath,
    pub view: LogicalPath,
    pub slots: Vec<Drawn>,
}

/// Why a page cannot be drawn at all.
#[derive(Debug, thiserror::Error)]
pub enum PageError {
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error("{path} is not a ui/binding/v1 record")]
    NotABinding { path: LogicalPath },
    #[error("{path} is not a valid binding: {source}")]
    Invalid { path: LogicalPath, source: UiError },
    #[error("{path} cannot be drawn: {source}")]
    View {
        path: LogicalPath,
        source: BindingError,
    },
}

impl PageError {
    pub fn code(&self) -> Code {
        match self {
            Self::Read(error) => error.code(),
            Self::NotABinding { .. } => Code::UiNotABinding,
            Self::Invalid { source, .. } => source.code(),
            Self::View { source, .. } => source.code(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        let fields = match self {
            Self::Read(error) => return error.refusal("ui"),
            Self::NotABinding { path } | Self::Invalid { path, .. } => Fields::at(path),
            Self::View { path, source } => Fields {
                path: Some(path.clone()),
                ..source.fields()
            },
        };
        Refusal::new(self.code(), self, &fields)
    }
}

impl Board {
    /// The binding at `path`, read from the working tree.
    pub fn binding(&self, path: &LogicalPath) -> Result<Binding, PageError> {
        let mut view = self.working_view()?;
        self.binding_in(&mut view, path)
    }

    /// The binding at `path` with each slot its view names read, all in one
    /// snapshot of the working tree. A slot that cannot be drawn carries
    /// its refusal, so the rest of the page still draws.
    pub fn page(&self, path: &LogicalPath) -> Result<Page, PageError> {
        let mut view = self.working_view()?;
        let binding = self.binding_in(&mut view, path)?;
        let read = &mut |target: &LogicalPath| {
            let read = (self.locate_in(&mut view, target))
                .and_then(|location| read_at(&mut view, location, target, true))
                .map_err(|error| match error.code() {
                    Code::PathNotFound => (Code::RefDangling, "does not resolve".to_owned()),
                    code => (code, error.to_string()),
                })?;
            (read.record).map_err(|_| (Code::RefWrongType, "is not a record".to_owned()))
        };
        let at = |source| PageError::View {
            path: path.clone(),
            source,
        };
        let slots = view_of(&binding, read).map_err(at)?;
        let slots = slots
            .into_iter()
            .map(|(name, kind)| {
                let fill = binding.slots.get(&name);
                let drawn = match fill {
                    Some(fill) => draw(read, &name, kind, fill),
                    None => Err(BindingError::Mismatch {
                        view: binding.view.clone(),
                        expected: vec![name.clone()],
                        found: Vec::new(),
                    }),
                };
                let reference = match fill {
                    Some(Fill::Ref(target)) => Some(target.clone()),
                    _ => None,
                };
                let (data, failed) = match drawn {
                    Ok(data) => (Some(data), None),
                    Err(error) => (None, Some(Failed::from(&error))),
                };
                Drawn {
                    name,
                    kind: kind.name(),
                    reference,
                    data,
                    failed,
                }
            })
            .collect();
        Ok(Page {
            binding: path.clone(),
            view: binding.view,
            slots,
        })
    }

    fn binding_in(
        &self,
        view: &mut Resolver<'_, '_>,
        path: &LogicalPath,
    ) -> Result<Binding, PageError> {
        if !path.as_str().starts_with("/bindings/") {
            return Err(PageError::NotABinding { path: path.clone() });
        }
        let location = self.locate_in(view, path)?;
        let read = read_at(view, location, path, true)?;
        let value = read
            .record
            .ok()
            .filter(|value| value.get("$type").and_then(Value::as_str) == Some(BINDING_TYPE))
            .ok_or_else(|| PageError::NotABinding { path: path.clone() })?;
        Binding::from_value(&value).map_err(|source| PageError::Invalid {
            path: path.clone(),
            source,
        })
    }
}

/// Reads a blob at a location in its own board.
pub(crate) type ReadBlob<'a> =
    dyn FnMut(&Location, ObjectId) -> Result<Option<Contents>, GitError> + 'a;

/// The first way `binding`, or when `None` the binding at `path`, does not fit
/// its view, with each path found by `resolve` and read by `read_blob`.
pub(crate) fn fitted(
    (path, binding): (&LogicalPath, Option<Binding>),
    read_blob: &mut ReadBlob<'_>,
    resolve: &mut dyn FnMut(&LogicalPath) -> Result<Location, ResolveError>,
) -> Option<(Code, Detail)> {
    let read = &mut |path: &LogicalPath| {
        let location = (resolve(path))
            .map_err(|error| (Code::RefDangling, format!("does not resolve: {error}")))?;
        let reason = match location.object {
            Object::Blob(blob) => match read_blob(&location, blob) {
                Ok(None) => "is too large to read",
                Ok(Some(read)) => match classify(&location.path, Some(&read.bytes)) {
                    Classified {
                        content: Content::Record { value, .. },
                        code: None,
                    } => return Ok(value),
                    _ => "is not a record",
                },
                Err(_) => "cannot be read",
            },
            Object::Directory => "is not a record",
        };
        Err((Code::RefWrongType, reason.to_owned()))
    };
    let binding = binding.or_else(|| Binding::from_value(&read(path).ok()?).ok())?;
    let error = fit(&binding, read).err()?;
    let fields = error.fields();
    let detail = Detail {
        pointer: fields.pointer,
        reason: format!("does not fit its view: {error}"),
        failures: 1,
        target: fields.target,
        ..Detail::default()
    };
    Some((error.code(), detail))
}

/// Checks that `binding` fills exactly the slots its view names, and that
/// each slot's data or referenced record satisfies its kind.
fn fit(binding: &Binding, read: &mut Read<'_>) -> Result<(), BindingError> {
    let slots = view_of(binding, read)?;
    let expected: BTreeSet<&String> = slots.iter().map(|(name, _)| name).collect();
    let found: BTreeSet<&String> = binding.slots.keys().collect();
    if expected != found {
        return Err(BindingError::Mismatch {
            view: binding.view.clone(),
            expected: expected.into_iter().cloned().collect(),
            found: found.into_iter().cloned().collect(),
        });
    }
    for (name, kind) in &slots {
        if let Some(fill) = binding.slots.get(name) {
            draw(read, name, *kind, fill)?;
        }
    }
    Ok(())
}

/// The slots of the view `binding` names, in order.
fn view_of(
    binding: &Binding,
    read: &mut Read<'_>,
) -> Result<Vec<(String, ComponentKind)>, BindingError> {
    let value = target(read, "/view", &binding.view, &[VIEW_TYPE])?;
    let parsed = View::from_value(&value).map_err(BindingError::View)?;
    Ok(parsed
        .slots
        .into_iter()
        .map(|slot| (slot.name, slot.kind))
        .collect())
}

/// The value a slot draws, checked against its kind's schema.
fn draw(
    read: &mut Read<'_>,
    name: &str,
    kind: ComponentKind,
    fill: &Fill,
) -> Result<Value, BindingError> {
    let (value, pointer) = match fill {
        Fill::Data(data) => (data.clone(), format!("/slots/{name}/data")),
        Fill::Ref(path) => {
            let pointer = format!("/slots/{name}/ref");
            (target(read, &pointer, path, kind.draws())?, pointer)
        }
    };
    let invalid = |reason: String| BindingError::Invalid {
        pointer: pointer.clone(),
        slot: name.to_owned(),
        kind: kind.name(),
        reason,
    };
    let schema = Schema::compile(kind.schema()).map_err(|error| invalid(error.to_string()))?;
    schema
        .validate(&value)
        .map_err(|error| invalid(error.reason()))?;
    kind.drawn(value).map_err(|error| BindingError::Invalid {
        pointer: format!("{pointer}/patch"),
        slot: name.to_owned(),
        kind: kind.name(),
        reason: error.to_string(),
    })
}

/// The record at `path`, which must be of one of `types`, where `*` is any
/// type.
fn target(
    read: &mut Read<'_>,
    pointer: &str,
    path: &LogicalPath,
    types: &[&str],
) -> Result<Value, BindingError> {
    let refused = |(code, reason)| BindingError::Target {
        pointer: pointer.to_owned(),
        target: path.clone(),
        code,
        reason,
    };
    if types.is_empty() {
        return Err(refused((
            Code::RefWrongType,
            "is a reference, and this kind takes only data".to_owned(),
        )));
    }
    let value = read(path).map_err(refused)?;
    let found = value.get("$type").and_then(Value::as_str).unwrap_or("");
    if types.contains(&"*") || types.contains(&found) {
        return Ok(value);
    }
    Err(refused((
        Code::RefWrongType,
        format!("is a {found} record, and this draws {}", types.join(" or ")),
    )))
}

#[cfg(test)]
mod tests;

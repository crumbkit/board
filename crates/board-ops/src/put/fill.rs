//! A command-line put's fills and drops: a field its type lets Board fill
//! is filled when the caller left it out, and a field its schema does not
//! name is dropped, before the schema check (docs/architecture/writes.md,
//! Fills and drops). Both change only the record in memory. A plugin's put
//! through its host opens the board again, without them.

use std::borrow::Cow;
use std::ffi::OsStr;

use board_model::{LogicalPath, RecordType, canonical_json, parse_json, utc_seconds};
use board_plugin::shape::session_id;
use board_validate::Schema;
use serde_json::{Map, Value, json};

use super::Refused;
use crate::board::Board;
use crate::host::Settings;

/// A field of a record type that `board put` fills when the caller leaves
/// it out. A built-in plugin declares its fills with [`crate::Plugin::fills`].
#[derive(Debug)]
pub struct Fill {
    /// The record type, such as `plans/question/v1`.
    pub record_type: &'static str,
    /// The top-level field filled, such as `asked`.
    pub field: &'static str,
    pub with: With,
}

/// What a fill puts in its field.
#[derive(Debug)]
pub enum With {
    /// `{"session": <id>}`, the put's session, when it has one.
    Session,
    /// The put's time, in UTC to the second.
    Now,
    /// This text.
    Text(&'static str),
    /// What the function makes of the record and the put's settings: a
    /// value, nothing to fill, or why it cannot.
    Derived(Derive),
}

/// A fill computed from the record, such as a review's patch ids.
pub type Derive = fn(&Map<String, Value>, &Settings) -> Result<Option<Value>, String>;

/// What a command-line put knows that a record may leave out: its session,
/// and the settings a derived fill reads, `now` and `caller` among them.
#[derive(Clone, Debug)]
pub struct Mending {
    pub session: Option<String>,
    pub settings: Settings,
}

impl Mending {
    /// The session `BOARD_SESSION` names, or else the Claude Code host
    /// session `CLAUDE_CODE_HOST_SESSION_ID` names. Each is used only when
    /// it is a `SessionId`, `local_` then lowercase hex and `-`, so an
    /// invalid `BOARD_SESSION` does not hide a valid host session.
    pub fn session(board: Option<&OsStr>, host: Option<&OsStr>) -> Option<String> {
        [board, host]
            .into_iter()
            .flatten()
            .filter_map(OsStr::to_str)
            .find(|id| session_id(id))
            .map(str::to_owned)
    }
}

/// The fills and drops a board's command-line puts make.
#[derive(Clone, Debug)]
pub(crate) struct Mend {
    mending: Mending,
    fills: Vec<&'static Fill>,
}

/// What a put filled and dropped in one record, and the batch line it came
/// from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mends {
    pub line: Option<usize>,
    /// The pointer of each field filled.
    pub filled: Vec<String>,
    pub dropped: Vec<Dropped>,
    /// Why each fill that could not be made was not, after the pointer of
    /// its field, such as `/by: the put has no session; set BOARD_SESSION`.
    pub unfilled: Vec<String>,
}

/// A field dropped: its pointer, and the pointer of the field the type
/// names one edit away from it, when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub pointer: String,
    pub near: Option<String>,
}

/// What a put filled and dropped in the record it wrote at `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mended {
    pub path: LogicalPath,
    pub mends: Mends,
}

impl Mends {
    /// These mends of the record written at `path`, unless there are none.
    pub(crate) fn at(&self, path: &LogicalPath) -> Option<Mended> {
        (!self.filled.is_empty() || !self.dropped.is_empty() || !self.unfilled.is_empty()).then(
            || Mended {
                path: path.clone(),
                mends: self.clone(),
            },
        )
    }
}

/// A record mended: its bytes, what was filled and dropped, and each field
/// kept though its type does not name it, because it is one edit from a
/// field the record lacks, so the check refuses it.
pub(crate) struct Applied<'a> {
    pub(crate) bytes: Cow<'a, [u8]>,
    pub(crate) mends: Mends,
    pub(crate) kept: Vec<Kept>,
}

/// A field kept for the check to refuse, and the field it is one edit from.
pub(crate) struct Kept {
    pointer: String,
    near: String,
}

impl<'a> Applied<'a> {
    /// `bytes` as given, with nothing filled or dropped.
    pub(crate) const fn none(bytes: &'a [u8]) -> Self {
        Self {
            bytes: Cow::Borrowed(bytes),
            mends: Mends {
                line: None,
                filled: Vec::new(),
                dropped: Vec::new(),
                unfilled: Vec::new(),
            },
            kept: Vec::new(),
        }
    }

    /// `refused`, naming what was dropped, kept and could not be filled, so
    /// a refusal after a drop says what the record lost and what to rename.
    pub(crate) fn refused(&self, refused: Refused) -> Refused {
        let Mends {
            dropped, unfilled, ..
        } = &self.mends;
        let dropped = (!dropped.is_empty()).then(|| {
            let pointers: Vec<&str> = dropped.iter().map(|field| field.pointer.as_str()).collect();
            format!(
                "before the check, board put dropped {}, which its type does not name",
                pointers.join(", ")
            )
        });
        let kept = self.kept.iter().map(|kept| {
            format!(
                "board put kept {}, which its type does not name, since it is one edit from {}, which the record lacks; rename it",
                kept.pointer, kept.near
            )
        });
        let unfilled = unfilled
            .iter()
            .map(|why| format!("board put could not fill {why}"));
        let notes: Vec<String> = dropped.into_iter().chain(kept).chain(unfilled).collect();
        if notes.is_empty() {
            return refused;
        }
        Refused::Mended {
            refused: Box::new(refused),
            notes,
        }
    }
}

impl Mend {
    /// `bytes` with each fill of its type made on a field it lacks, then
    /// each field the schema `schema` finds does not name removed. Input
    /// that is not a JSON object with a `$type` is left as it is, for the
    /// check to refuse.
    pub(crate) fn apply<'a, 's>(
        &self,
        bytes: &'a [u8],
        schema: impl FnOnce(&RecordType) -> Option<&'s Schema>,
    ) -> Applied<'a> {
        let unchanged = || Applied::none(bytes);
        let Ok(mut value) = parse_json(bytes) else {
            return unchanged();
        };
        let Some(record_type) = (value.get("$type").and_then(Value::as_str))
            .and_then(|text| text.parse::<RecordType>().ok())
        else {
            return unchanged();
        };
        let Some(record) = value.as_object_mut() else {
            return unchanged();
        };
        let (filled, unfilled) = self.fill(&record_type, record);
        let (dropped, kept) = if record_type.is_known() {
            (Vec::new(), Vec::new())
        } else {
            schema(&record_type).map_or_default(|schema| dropped(schema, &mut value))
        };
        let bytes = if filled.is_empty() && dropped.is_empty() {
            Cow::Borrowed(bytes)
        } else {
            Cow::Owned(canonical_json(&value).into_bytes())
        };
        Applied {
            bytes,
            mends: Mends {
                line: None,
                filled,
                dropped,
                unfilled,
            },
            kept,
        }
    }

    /// Makes each fill of `record_type` that `record` lacks, and returns
    /// the pointers filled and why each fill that could not be made was not.
    fn fill(
        &self,
        record_type: &RecordType,
        record: &mut Map<String, Value>,
    ) -> (Vec<String>, Vec<String>) {
        let (mut filled, mut unfilled) = (Vec::new(), Vec::new());
        let fills = (self.fills.iter()).filter(|fill| fill.record_type == record_type.as_str());
        for fill in fills {
            if record.contains_key(fill.field) {
                continue;
            }
            let settings = &self.mending.settings;
            let made = match &fill.with {
                With::Session => (self.mending.session.as_ref())
                    .map(|id| Some(json!({"session": id})))
                    .ok_or_else(|| {
                        format!(
                            "the put has no session; set BOARD_SESSION to your local_ session id, or give {}",
                            fill.field
                        )
                    }),
                With::Now => Ok(Some(json!(utc_seconds(settings.now)))),
                With::Text(text) => Ok(Some(json!(text))),
                With::Derived(derive) => derive(record, settings),
            };
            let value = made.unwrap_or_else(|why| {
                unfilled.push(format!("/{}: {why}", fill.field));
                None
            });
            if let Some(value) = value {
                record.insert(fill.field.to_owned(), value);
                filled.push(format!("/{}", fill.field));
            }
        }
        (filled, unfilled)
    }
}

/// Removes from `value` each field `schema` does not name, and returns
/// them, and each field kept because it is one edit from a field the
/// object lacks, which is most likely a misspelling of it.
fn dropped(schema: &Schema, value: &mut Value) -> (Vec<Dropped>, Vec<Kept>) {
    let (mut dropped, mut kept) = (Vec::new(), Vec::new());
    for field in schema.unexpected(value) {
        let Some(object) = value.pointer_mut(&field.at).and_then(Value::as_object_mut) else {
            continue;
        };
        if let (Some(near), Some(pointer)) = (&field.near, field.near_pointer())
            && !object.contains_key(near)
        {
            kept.push(Kept {
                pointer: field.pointer(),
                near: pointer,
            });
            continue;
        }
        if object.remove(&field.name).is_some() {
            dropped.push(Dropped {
                pointer: field.pointer(),
                near: field.near_pointer(),
            });
        }
    }
    (dropped, kept)
}

impl Board {
    /// Makes every command-line put of this board, and of a board it puts
    /// through a mount, fill the fields that the built-in plugins declare
    /// and drop the fields a schema does not name. Call it after
    /// [`Board::build_in`].
    pub fn mend(&mut self, mending: Mending) {
        let fills = (self.built_in.0.iter())
            .flat_map(|plugin| plugin.fills())
            .collect();
        self.mend = Some(Mend { mending, fills });
    }

    /// [`Board::mend`] with `fills` in place of the built-in plugins'.
    #[cfg(test)]
    pub(crate) fn mend_with(&mut self, mending: Mending, fills: Vec<&'static Fill>) {
        self.mend = Some(Mend { mending, fills });
    }
}

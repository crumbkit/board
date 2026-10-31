//! The folder a `--new` put writes in: the one it names, or the one its
//! record's type declares in its schema's `x-board-folder`, with each
//! `{name}` filled from `--var` (docs/architecture/paths/writes.md).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use board_model::{LogicalPath, RecordType};
use serde_json::Value;

use super::new::{drawn, folder_text};
use super::{PutError, PutReport, Refused, Schemas};
use crate::board::Board;

/// The value each `--var` gives a placeholder, by its name.
pub type Vars = BTreeMap<String, String>;

impl Board {
    /// Writes the record `bytes` at a new name under `folder`, or with no
    /// folder, under the one its type declares with `vars` filled in.
    pub fn put_new_waiting(
        &self,
        folder: Option<&LogicalPath>,
        vars: &Vars,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let folder = self.folder(folder, vars, bytes)?;
        self.put_named(&folder, bytes, wait, &mut drawn())
    }

    /// `folder`, which takes no `vars`, or the folder the type of the
    /// record `bytes` declares, filled from `vars`.
    pub(crate) fn folder(
        &self,
        folder: Option<&LogicalPath>,
        vars: &Vars,
        bytes: &[u8],
    ) -> Result<LogicalPath, PutError> {
        if let Some(folder) = folder {
            return match vars.keys().next() {
                Some(var) => Err(unused(var, &format!("{}/", folder.as_str()))),
                None => Ok(folder.clone()),
            };
        }
        let (record_type, declared) = self.declared(bytes)?;
        let text = filled(&record_type, &declared, vars)?;
        let folder: LogicalPath = folder_text(&text).parse()?;
        if text.ends_with('/') {
            Ok(folder)
        } else {
            Err(PutError::NotAFolder { folder })
        }
    }

    /// The type of the record `bytes` and the folder its schema declares.
    /// The record is checked first, so one that no put admits keeps its code.
    fn declared(&self, bytes: &[u8]) -> Result<(RecordType, String), PutError> {
        let mut schemas = Schemas::default();
        let admitted = (self.check_record(&mut schemas, bytes))
            .map_err(|refused| PutError::Unplaced { refused })?;
        let record_type: RecordType = (admitted.value.get("$type").and_then(Value::as_str))
            .and_then(|text| text.parse().ok())
            .ok_or(PutError::Unplaced {
                refused: Refused::NoType,
            })?;
        let declared = (schemas.held.get(&record_type))
            .and_then(|schema| schema.as_ref().ok())
            .and_then(|schema| schema.document().get("x-board-folder")?.as_str());
        match declared {
            Some(declared) => Ok((record_type, declared.to_owned())),
            None => Err(PutError::NoFolder { record_type }),
        }
    }
}

/// `declared` with each `{name}` replaced by the value `vars` gives it. A
/// `{` with no `}` after it is kept as written.
pub(crate) fn filled(
    record_type: &RecordType,
    declared: &str,
    vars: &Vars,
) -> Result<String, PutError> {
    let (mut text, mut rest, mut used) = (String::new(), declared, BTreeSet::new());
    while let Some((before, after)) = rest.split_once('{')
        && let Some((name, after)) = after.split_once('}')
    {
        let value = vars.get(name).ok_or_else(|| PutError::FolderUnfilled {
            record_type: record_type.clone(),
            folder: declared.to_owned(),
            var: name.to_owned(),
        })?;
        text.push_str(before);
        text.push_str(value);
        used.insert(name);
        rest = after;
    }
    text.push_str(rest);
    match vars.keys().find(|var| !used.contains(var.as_str())) {
        Some(var) => Err(unused(var, declared)),
        None => Ok(text),
    }
}

/// The refusal of `--var var`, which names no placeholder of `folder`.
fn unused(var: &str, folder: &str) -> PutError {
    PutError::VarUnused {
        var: var.to_owned(),
        folder: folder.to_owned(),
    }
}

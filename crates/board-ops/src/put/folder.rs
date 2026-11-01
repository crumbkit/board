//! The folder a `--new` put with no folder writes in: the one its record's
//! type declares in its schema's `x-board-folder`, with each `{name}`
//! filled from `--var` (docs/architecture/paths/writes.md).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use board_model::{Content, LogicalPath, RecordType};

use super::new::{drawn, folder_text};
use super::{PutError, PutReport, Schemas, checked};
use crate::board::Board;

/// The value each `--var` gives a placeholder, by its name.
pub type Vars = BTreeMap<String, String>;

impl Board {
    /// Writes the record `bytes` at a new name under the folder its type
    /// declares, with `vars` filled in.
    pub fn put_declared_waiting(
        &self,
        vars: &Vars,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let folder = self.declared_folder(vars, bytes)?;
        self.put_named(&folder, bytes, wait, &mut drawn())
    }

    /// The folder the type of the record `bytes` declares, filled from
    /// `vars`.
    pub(crate) fn declared_folder(
        &self,
        vars: &Vars,
        bytes: &[u8],
    ) -> Result<LogicalPath, PutError> {
        let (record_type, declared) = self.declared(bytes)?;
        let text = filled(&record_type, &declared, vars)?;
        Ok(folder_text(&text).parse()?)
    }

    /// The type of the record `bytes` and the folder its schema declares.
    /// The record is checked first, so one that no put admits keeps its code.
    /// Known cost: the put that follows checks the record and compiles its
    /// schema again under its lock, because each name it draws takes the lock
    /// afresh.
    fn declared(&self, bytes: &[u8]) -> Result<(RecordType, String), PutError> {
        let unplaced = |refused| PutError::Unplaced { refused };
        let classified = checked(bytes).map_err(unplaced)?;
        let record_type = match &classified.content {
            Content::Record { record_type, .. } | Content::Pointer { record_type, .. } => {
                Some(record_type.clone())
            }
            _ => None,
        };
        let mut schemas = Schemas::default();
        (self.admit(&mut schemas, classified)).map_err(|fault| unplaced(fault.into()))?;
        let record_type =
            record_type.expect("`admit` refuses every content but a record or a pointer");
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
        Some(var) => Err(PutError::VarUnused {
            var: var.clone(),
            folder: declared.to_owned(),
        }),
        None => Ok(text),
    }
}

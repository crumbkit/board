//! `board check`: every entry of the working tree, parsed and validated, with
//! its references and pointers followed (docs/architecture.md, Records;
//! docs/architecture/paths.md; docs/architecture/index.md), as a snapshot
//! decides it.

use std::path::Path;

use board_index::{Fallback, Index, IndexError};
use board_model::{Change, Code, Fields, Kind, Location, LogicalPath, RecordType, Source};
use board_tree::{ResolveError, Status};

use crate::board::Board;
use crate::index::{RefreshError, Refreshed};

/// One entry as `check` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// The working tree, or the mounted commit the entry is in.
    pub source: Source,
    /// The path inside its source.
    pub path: String,
    /// The display path from the root board.
    pub logical: Option<LogicalPath>,
    pub kind: Kind,
    pub status: Status,
    /// Where a degraded entry fails and why, when Board knows.
    pub detail: Option<Detail>,
    pub change: Change,
    /// The schema file a record of an unknown type needs, from its source's root.
    pub schema: Option<String>,
}

/// Where an entry fails, as a JSON pointer when it is a field, why, and how
/// many failures of that kind the entry has, of which this is the first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub pointer: Option<String>,
    pub reason: String,
    pub failures: u64,
}

/// An entry's code, with its detail when Board knows one.
pub(crate) type Failure = (Code, Option<Detail>);

/// The first failing reference of a record, and how many fail.
#[derive(Default)]
pub(crate) struct Tally {
    first: Option<(Code, Detail)>,
    failures: u64,
}

impl Tally {
    pub(crate) fn add(&mut self, pointer: &str, failure: Option<&(Code, String)>) {
        let Some((code, reason)) = failure else {
            return;
        };
        self.failures += 1;
        self.first.get_or_insert_with(|| {
            let detail = Detail {
                pointer: Some(pointer.to_owned()),
                reason: reason.clone(),
                failures: 0,
            };
            (*code, detail)
        });
    }

    pub(crate) fn first(self) -> Option<(Code, Detail)> {
        let failures = self.failures;
        self.first
            .map(|(code, detail)| (code, Detail { failures, ..detail }))
    }
}

/// Where a reference led, and its code and reason when it fails.
pub(crate) type Followed = (Option<Location>, Option<(Code, String)>);

/// Follows a reference to `target`, and says why it fails, if it does: it leads nowhere, or,
/// when it `accepts` only some record types, to anything else. `type_of`
/// gives the record type at a location.
pub(crate) fn follow<E>(
    target: &LogicalPath,
    accepts: Option<&[RecordType]>,
    resolved: Result<Location, ResolveError>,
    type_of: impl FnOnce(&Location) -> Result<Option<RecordType>, E>,
) -> Result<Followed, E> {
    let location = match resolved {
        Err(error) => return Ok((None, Some((Code::RefDangling, error.to_string())))),
        Ok(location) => location,
    };
    let failure = match accepts {
        Some(accepts) if !type_of(&location)?.is_some_and(|found| accepts.contains(&found)) => {
            let types: Vec<&str> = accepts.iter().map(RecordType::as_str).collect();
            let reason = format!("{target} is not a record of type {}", types.join(" or "));
            Some((Code::RefWrongType, reason))
        }
        _ => None,
    };
    Ok((Some(location), failure))
}

/// Every entry `check` looked at: the root's sorted by path, then each
/// mounted source's, by route. And why the index was in memory if it was.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckReport {
    pub entries: Vec<Checked>,
    pub memory: Option<Fallback>,
    /// The work the check did; `hashed` counts files hashed to resolve
    /// references and pointers too.
    pub work: Refreshed,
}

impl CheckReport {
    /// Whether any entry is degraded, which makes `check` fail.
    pub fn degraded(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.status, Status::Degraded(_)))
    }
}

impl Board {
    /// Checks the working tree against the index kept under `cache`, or one
    /// in memory when there is none.
    pub fn check(
        &self,
        scope: Option<&LogicalPath>,
        cache: Option<&Path>,
    ) -> Result<CheckReport, CheckError> {
        let index = Index::open(cache, self.repository().path())?;
        Ok(self.snapshot(index, scope)?.report)
    }
}

/// A pointer that does not resolve, for the reason `error` gives.
pub(crate) fn unresolved(error: &ResolveError) -> (Code, Detail) {
    let detail = Detail {
        pointer: None,
        reason: error.to_string(),
        failures: 1,
    };
    (error.code(), detail)
}

/// A check that could not run.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error(transparent)]
    Scope { source: ResolveError },
    #[error(transparent)]
    Refresh {
        #[from]
        source: RefreshError,
    },
    #[error(transparent)]
    Index {
        #[from]
        source: IndexError,
    },
}

impl CheckError {
    pub fn code(&self) -> Code {
        match self {
            Self::Scope { source } => source.code(),
            Self::Refresh { source } => source.code(),
            Self::Index { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Scope { source } => source.fields(),
            Self::Refresh { .. } | Self::Index { .. } => Fields::default(),
        }
    }
}

#[cfg(test)]
mod tests;

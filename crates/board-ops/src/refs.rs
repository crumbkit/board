//! `board refs`: the references a record makes and the records that make
//! references to it (docs/architecture.md, Commands). Both come from what a
//! check decides, so each reference is resolved once, as the tree is now.

use std::collections::BTreeMap;
use std::path::Path;

use board_index::{Fallback, Index};
use board_model::{Code, Fields, Location, LogicalPath, Source};
use board_tree::{ResolveError, Status};

use crate::board::Board;
use crate::check::{CheckError, Referenced, View};
use crate::read::{ReadError, display, join};

/// One reference the record makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    /// Where it is in the record, as a JSON pointer.
    pub pointer: String,
    pub target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub location: Option<Location>,
    pub status: Status,
}

/// One record that makes references to the object, however it names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub source: Source,
    pub path: String,
    pub logical: Option<LogicalPath>,
    /// The JSON pointers in that record whose references reach the object.
    pub pointers: Vec<String>,
}

/// Where a path that names nothing yet would be, beneath the deepest
/// ancestor that resolves: the physical path a file there would have, by
/// put's resolution. Put may still refuse it, for instance through a mount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    /// The segments past the ancestor.
    pub segments: Vec<String>,
    /// The physical path, in the ancestor's board.
    pub path: String,
    pub display: LogicalPath,
}

/// References from and to the object at one path. For a path that names
/// nothing, from [`Board::refs_or_missing`], `location` and `display` are
/// the deepest ancestor's, and there are no references.
#[derive(Clone, Debug, PartialEq)]
pub struct RefsReport {
    pub location: Location,
    pub display: LogicalPath,
    pub from: Vec<Outgoing>,
    pub to: Vec<Incoming>,
    pub memory: Option<Fallback>,
    pub missing: Option<Missing>,
}

impl Board {
    /// Resolves `path`, then checks the board against the index kept under
    /// `cache` and reports the references the object there makes and those
    /// that reach it. Two references reach it when they resolve to its
    /// identity, its board and physical path, whatever path they name.
    pub fn refs(&self, path: &LogicalPath, cache: Option<&Path>) -> Result<RefsReport, RefsError> {
        let location = self
            .repository()
            .resolve(&Source::WorkingTree, path)
            .map_err(refused)?;
        let mut index = Index::open(cache, self.repository().path()).map_err(CheckError::from)?;
        let (checked, references) = self.decide(&mut index, None, View::Skip)?;
        let same = |source: &Source, physical: &str| {
            *source == location.board.source && physical == location.path
        };
        let from = references
            .iter()
            .filter(|referenced| same(&referenced.source, &referenced.path))
            .map(|referenced| Outgoing {
                pointer: referenced.reached.pointer.clone(),
                target: referenced.reached.target.clone(),
                location: referenced.reached.location.clone(),
                status: referenced.reached.code.map_or(Status::Ok, Status::Degraded),
            })
            .collect();
        let mut to: BTreeMap<(String, String), Incoming> = BTreeMap::new();
        for referenced in references.iter().filter(|referenced| {
            referenced
                .reached
                .location
                .as_ref()
                .is_some_and(|reached| same(&reached.board.source, &reached.path))
        }) {
            let Referenced {
                source,
                path,
                logical,
                reached,
            } = referenced;
            to.entry((board_index::source_name(source), path.clone()))
                .or_insert_with(|| Incoming {
                    source: source.clone(),
                    path: path.clone(),
                    logical: logical.clone(),
                    pointers: Vec::new(),
                })
                .pointers
                .push(reached.pointer.clone());
        }
        Ok(RefsReport {
            display: display(&location, path),
            location,
            from,
            to: to.into_values().collect(),
            memory: checked.memory,
            missing: None,
        })
    }

    /// As [`Board::refs`], except that a path that names nothing reports
    /// its deepest ancestor that resolves and, in `missing`, the physical
    /// path a file there would have by put's resolution. A path ending
    /// `.json` whose stem names something is still not found, since it
    /// names that entry by its file.
    pub fn refs_or_missing(
        &self,
        path: &LogicalPath,
        cache: Option<&Path>,
    ) -> Result<RefsReport, RefsError> {
        let not_found = || refused(ResolveError::NotFound { path: path.clone() });
        match self.refs(path, cache) {
            Err(RefsError::Read {
                source:
                    ReadError::Resolve {
                        source: ResolveError::NotFound { .. },
                    },
            }) => {}
            answered => return answered,
        }
        let stem = path.as_str().strip_suffix(".json");
        if let Some(Ok(stem)) = stem.map(str::parse::<LogicalPath>)
            && self
                .repository()
                .resolve(&Source::WorkingTree, &stem)
                .is_ok()
        {
            return Err(not_found());
        }
        let ancestor = self.repository().ancestor(path).map_err(refused)?;
        let physical = ancestor.path().ok_or_else(not_found)?;
        let display = display(&ancestor.location, path);
        let named = ancestor
            .rest
            .iter()
            .try_fold(display.clone(), |parent, segment| join(&parent, segment))
            .ok_or_else(not_found)?;
        Ok(RefsReport {
            missing: Some(Missing {
                display: named,
                segments: ancestor.rest,
                path: physical,
            }),
            location: ancestor.location,
            display,
            from: Vec::new(),
            to: Vec::new(),
            memory: None,
        })
    }
}

fn refused(source: ResolveError) -> RefsError {
    RefsError::Read {
        source: ReadError::Resolve { source },
    }
}

/// A `refs` that could not run.
#[derive(Debug, thiserror::Error)]
pub enum RefsError {
    #[error(transparent)]
    Read { source: ReadError },
    #[error(transparent)]
    Check {
        #[from]
        source: CheckError,
    },
}

impl RefsError {
    pub fn code(&self) -> Code {
        match self {
            Self::Read { source } => source.code(),
            Self::Check { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Read { source } => source.fields(),
            Self::Check { source } => source.fields(),
        }
    }
}

#[cfg(test)]
mod tests;

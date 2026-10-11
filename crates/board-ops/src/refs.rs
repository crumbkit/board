//! `board refs`: the references a record makes and the records that make
//! references to it (docs/architecture/index.md). Both come from what a
//! snapshot decides, so each reference is resolved once, as the tree is now,
//! and one snapshot answers every path a run asks about.

use std::collections::BTreeMap;
use std::path::Path;

use board_index::Fallback;
use board_model::{Code, Location, LogicalPath, Object, Refusal, RefusalCode, Source};
use board_tree::{GitError, ResolveError, Resolver, Status};
use serde_json::Value;

use crate::board::Board;
use crate::check::CheckError;
use crate::read::{ReadError, display, join, read_at};
use crate::snapshot::{Referenced, Snapshot};

/// One reference the record makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    /// Where it is in the record, as a JSON pointer.
    pub pointer: String,
    pub target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub location: Option<Location>,
    pub status: Status,
    /// What the target holds at the fields the run asked for. `None` when
    /// the run asked for none, when the reference resolves to nothing, and
    /// past the run's limit.
    pub fields: Option<Fielded>,
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
/// nothing, with [`Absent::Locate`], `location` and `display` are the
/// deepest ancestor's, and there are no references.
#[derive(Clone, Debug, PartialEq)]
pub struct RefsReport {
    pub location: Location,
    pub display: LogicalPath,
    pub from: Vec<Outgoing>,
    pub to: Vec<Incoming>,
    pub memory: Option<Fallback>,
    pub missing: Option<Missing>,
}

/// What `refs` answers for a path that names nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Absent {
    /// Refuse it, as `board::path::not_found`.
    Refuse,
    /// Report its deepest ancestor that resolves and, in `missing`, the
    /// physical path a file there would have by put's resolution. A path
    /// ending `.json` whose stem names something is still not found, since
    /// it names that entry by its file.
    Locate,
}

/// Where one asked path leads: an object, or nothing yet.
enum Found {
    At(Location),
    Missing(RefsReport),
}

/// What a reference's target holds at the pointers a run asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Fielded {
    /// Each pointer asked, in order, with the target record's value there,
    /// or `None` where the record has nothing. `None` as a whole when the
    /// target is not a record that could be read.
    pub values: Option<Vec<(String, Option<Value>)>>,
    /// Why the target could not be read, or the code check gives it.
    pub code: Option<Code>,
}

/// A run's state as it answers each path in turn.
struct Answering<'a> {
    board: &'a Board,
    view: Resolver<'a, 'static>,
    absent: Absent,
    fields: &'a [String],
    limit: usize,
    cache: Option<&'a Path>,
    snapshot: Option<Snapshot>,
}

impl Board {
    /// Each of `paths`, answered in order as the iterator is advanced: the
    /// references the object there makes and those that reach it, or why
    /// there is no answer. Two references reach it when they resolve to its
    /// identity, its board and physical path, whatever path they name. Every
    /// path resolves in one snapshot of the working tree, and the board is
    /// checked once, against the index kept under `cache`, when the first
    /// path that resolves needs it. A check that cannot run is
    /// [`RefsError::Check`] for that path, which every path after it needs
    /// too. For each of the first `limit` references a path makes that
    /// resolve, the target's values at each of `fields`, JSON pointers, are
    /// read as the answer is made. The outer error is a snapshot that cannot
    /// be taken.
    pub fn refs<'a>(
        &'a self,
        paths: &'a [LogicalPath],
        absent: Absent,
        fields: &'a [String],
        limit: Option<usize>,
        cache: Option<&'a Path>,
    ) -> Result<impl Iterator<Item = Result<RefsReport, RefsError>> + 'a, RefsError> {
        let view = self
            .repositories()
            .snapshot()
            .map_err(|source| refused(ResolveError::Git(source)))?;
        let mut answering = Answering {
            board: self,
            view,
            absent,
            fields,
            limit: limit.unwrap_or(usize::MAX),
            cache,
            snapshot: None,
        };
        Ok(paths.iter().map(move |path| answering.answer(path)))
    }
}

impl Answering<'_> {
    fn answer(&mut self, path: &LogicalPath) -> Result<RefsReport, RefsError> {
        let location = match self.found(path)? {
            Found::At(location) => location,
            Found::Missing(report) => return Ok(report),
        };
        let snapshot = if let Some(snapshot) = self.snapshot.take() {
            snapshot
        } else {
            let index = self.board.index(self.cache).map_err(CheckError::from)?;
            self.board.snapshot(index, None)?
        };
        let mut report = answer(&snapshot, location, path);
        if !self.fields.is_empty() {
            for outgoing in report.from.iter_mut().take(self.limit) {
                if let Some(location) = &outgoing.location {
                    let fielded = self.fielded(&snapshot, location, &outgoing.target);
                    outgoing.fields = Some(fielded);
                }
            }
        }
        self.snapshot = Some(snapshot);
        Ok(report)
    }

    /// Where `path` leads or, when `absent` says to locate it, where a file
    /// there would be.
    fn found(&mut self, path: &LogicalPath) -> Result<Found, RefsError> {
        let found = match self.board.locate_in(&mut self.view, path) {
            Ok(location) => return Ok(Found::At(location)),
            Err(ReadError::Resolve {
                source: ResolveError::NotFound { ancestor, .. },
            }) if self.absent == Absent::Locate => ancestor,
            Err(source) => return Err(RefsError::Read { source }),
        };
        let not_found = || {
            refused(ResolveError::NotFound {
                path: path.clone(),
                ancestor: found.clone(),
            })
        };
        let ancestor = self.view.ancestor(path).map_err(refused)?;
        let physical = ancestor.path().ok_or_else(not_found)?;
        let display = display(&ancestor.location, path);
        let named = ancestor
            .rest
            .iter()
            .try_fold(display.clone(), |parent, segment| join(&parent, segment))
            .ok_or_else(not_found)?;
        Ok(Found::Missing(RefsReport {
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
        }))
    }

    /// What the record at `location`, which `target` resolved to, holds at
    /// the run's fields, with the code `snapshot` gives it. It reads only
    /// the blob the walk listed and `snapshot` decided that code for: a file
    /// that resolved or reads as another blob changed after the walk, and is
    /// `refs::target_changed`, counted as `target_changed` in the trace.
    fn fielded(&self, snapshot: &Snapshot, location: &Location, target: &LogicalPath) -> Fielded {
        let degraded = snapshot
            .report
            .entries
            .iter()
            .find_map(|entry| match entry.status {
                Status::Degraded(code)
                    if entry.source == location.board.source && entry.path == location.path =>
                {
                    Some(code)
                }
                _ => None,
            });
        let walked = (snapshot.walked.iter())
            .find(|(source, _)| *source == location.board.source)
            .and_then(|(_, entries)| entries.iter().find(|entry| entry.path == location.path))
            .map(|entry| entry.blob);
        let changed = || {
            self.board.trace.borrow_mut().count("target_changed", 1);
            Code::RefsTargetChanged
        };
        let record = match location.object {
            Object::Blob(blob) if walked != Some(Some(blob)) => Err(changed()),
            _ => read_at(self.board.repositories(), location.clone(), target, false).map_err(
                |error| match error {
                    ReadError::Git {
                        source: GitError::Changed { .. },
                    } => changed(),
                    error => error.code(),
                },
            ),
        }
        .and_then(|report| {
            report.record.map_err(|refusal| match refusal.code {
                RefusalCode::Board(code) => code,
                RefusalCode::Plugin(_) => Code::RecordNotARecord,
            })
        });
        match record {
            Ok(record) => Fielded {
                values: Some(
                    (self.fields.iter())
                        .map(|field| (field.clone(), record.pointer(field).cloned()))
                        .collect(),
                ),
                code: degraded,
            },
            Err(code) => Fielded {
                values: None,
                code: Some(code),
            },
        }
    }
}

/// The references from and to the object at `location`, which `path`
/// resolved to, as `snapshot` decided them.
fn answer(snapshot: &Snapshot, location: Location, path: &LogicalPath) -> RefsReport {
    let same = |source: &Source, physical: &str| {
        *source == location.board.source && physical == location.path
    };
    let from = snapshot
        .references
        .iter()
        .filter(|referenced| same(&referenced.source, &referenced.path))
        .map(|referenced| Outgoing {
            pointer: referenced.reached.pointer.clone(),
            target: referenced.reached.target.clone(),
            location: referenced.reached.location.clone(),
            status: referenced.reached.code.map_or(Status::Ok, Status::Degraded),
            fields: None,
        })
        .collect();
    let mut to: BTreeMap<(String, String), Incoming> = BTreeMap::new();
    for referenced in snapshot.references.iter().filter(|referenced| {
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
    RefsReport {
        display: display(&location, path),
        location,
        from,
        to: to.into_values().collect(),
        memory: snapshot.report.memory,
        missing: None,
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

    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Read { source } => source.refusal("refs"),
            Self::Check { source } => source.refusal(),
        }
    }
}

#[cfg(test)]
mod tests;

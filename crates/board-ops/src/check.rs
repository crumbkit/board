//! `board check`: every entry of the working tree, parsed and validated, with
//! its references and pointers followed (docs/architecture.md, Records;
//! Pointers: links and mounts; The index). What depends only on content
//! comes from the index; whether a reference or pointer resolves depends on
//! the whole tree, so each check decides it again.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use board_index::{Fallback, Index, IndexError, Resolved, Storage, ValidationFact, Writer};
use board_model::{
    Change, Classified, Code, Content, Kind, Location, LogicalPath, Object, ObjectId, POINTER_TYPE,
    RecordType, Role, Source, Stat, classify,
};
use board_tree::{Entry, Repository, ResolveError, Resolver, Status};

use crate::board::Board;
use crate::index::{Indexed, Refresh, RefreshError, Refreshed, schema_of, schema_path};

/// One entry as `check` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    pub path: String,
    pub logical: Option<LogicalPath>,
    pub kind: Kind,
    pub status: Status,
    pub change: Change,
}

/// Every entry `check` looked at, sorted by path, and why the index was in
/// memory if it was.
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
        let mut index = Index::open(cache, self.repository().path())?;
        self.check_in(&mut index, scope)
    }

    /// Checks every entry of the working tree, or only those at or beneath
    /// `scope`, refreshing `index` first. An entry's status is the first
    /// problem found: its path, then parsing, then its schema, validation and
    /// references, or for a pointer its target. The `entries` view of
    /// `index` then shows exactly what was checked.
    pub fn check_in(
        &self,
        index: &mut Index,
        scope: Option<&LogicalPath>,
    ) -> Result<CheckReport, CheckError> {
        if let Some(scope) = scope {
            self.repository()
                .resolve(&Source::WorkingTree, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let writer = index.write()?;
        let refresh = self.refresh(&writer)?;
        let stats: HashMap<&str, (Stat, ObjectId)> = refresh
            .entries
            .iter()
            .filter_map(|entry| Some((entry.path.as_str(), (entry.stat?, entry.blob?))))
            .collect();
        // A file this refresh saw as it is now need not be hashed again.
        let hashed = Cell::new(refresh.work.hashed);
        let mut known = |path: &str, stat: &Stat| {
            let blob = stats
                .get(path)
                .filter(|(seen, _)| seen == stat)
                .map(|(_, blob)| *blob);
            if blob.is_none() {
                hashed.set(hashed.get() + 1);
            }
            blob
        };
        let decide = Decide {
            repository: self.repository(),
            writer: &writer,
            refresh: &refresh,
            resolver: RefCell::new(
                self.repository()
                    .resolver(&mut known)
                    .map_err(RefreshError::from)?,
            ),
        };
        let mut checked = Vec::new();
        let mut resolved = Vec::new();
        for entry in &refresh.entries {
            if !within(entry.logical.as_ref(), scope) {
                continue;
            }
            let fact = refresh.facts.get(&entry.path);
            let role = Role::of(&entry.path);
            let kind = match (role, fact) {
                (None, _) => Kind::File,
                (Some(_), Some(fact)) => fact.fact.kind,
                (Some(_), None) => Kind::Json,
            };
            let code = match entry.status {
                Status::Degraded(code) => Some(code),
                Status::Ok => decide.code(entry, role, fact)?,
            };
            let change = if refresh.head.get(&entry.path) == Some(&entry.blob) {
                Change::Committed
            } else {
                Change::Uncommitted
            };
            resolved.push(Resolved {
                path: entry.path.clone(),
                role,
                kind,
                code,
                change,
            });
            checked.push(Checked {
                path: entry.path.clone(),
                logical: entry.logical.clone(),
                kind,
                status: code.map_or(Status::Ok, Status::Degraded),
                change,
            });
        }
        drop(decide);
        writer.resolve(&resolved)?;
        writer.commit()?;
        let memory = match index.storage() {
            Storage::Memory(fallback) => Some(*fallback),
            Storage::File(_) => None,
        };
        Ok(CheckReport {
            entries: checked,
            memory,
            work: Refreshed {
                hashed: hashed.get(),
                ..refresh.work
            },
        })
    }
}

/// What deciding an entry's status reads.
struct Decide<'a, 'k> {
    repository: &'a Repository,
    writer: &'a Writer<'a>,
    refresh: &'a Refresh,
    /// One snapshot of the working tree for every resolution.
    resolver: RefCell<Resolver<'a, 'k>>,
}

impl Decide<'_, '_> {
    /// The code of an entry whose path is fine.
    fn code(
        &self,
        entry: &Entry,
        role: Option<Role>,
        fact: Option<&Indexed>,
    ) -> Result<Option<Code>, IndexError> {
        if role.is_none() {
            return Ok(None);
        }
        let Some(fact) = fact else {
            return Ok(Some(Code::RepoUnreadable));
        };
        if let Some(code) = fact.fact.code {
            return Ok(Some(code));
        }
        match fact.fact.kind {
            Kind::Pointer => Ok(entry
                .logical
                .as_ref()
                .and_then(|logical| self.resolve(logical).err())
                .map(|error| error.code())),
            Kind::Record => self.record(fact),
            Kind::Schema | Kind::Json | Kind::File => Ok(None),
        }
    }

    /// Checks a record against its schema and its references.
    fn record(&self, record: &Indexed) -> Result<Option<Code>, IndexError> {
        let Some(schema_path) = schema_path(record) else {
            return Ok(None);
        };
        let Some(schema) = schema_of(&self.refresh.facts, &schema_path) else {
            return Ok(Some(match self.refresh.facts.get(&schema_path) {
                Some(found) if found.role == Role::Schema => Code::SchemaInvalid,
                _ => Code::RecordUnknownType,
            }));
        };
        let references = match self.writer.validation(record.blob, schema)? {
            None => return Ok(Some(Code::RepoUnreadable)),
            Some(ValidationFact::Invalid { code, .. }) => return Ok(Some(code)),
            Some(ValidationFact::Valid(references)) => references,
        };
        for reference in references {
            let Ok(location) = self.resolve(&reference.target) else {
                return Ok(Some(Code::RefDangling));
            };
            let Some(accepts) = &reference.accepts else {
                continue;
            };
            match self.record_type(&location)? {
                Some(found) if accepts.contains(&found) => {}
                _ => return Ok(Some(Code::RefWrongType)),
            }
        }
        Ok(None)
    }

    fn resolve(&self, path: &LogicalPath) -> Result<Location, ResolveError> {
        self.resolver.borrow_mut().resolve(path)
    }

    /// The `$type` of the record at `location`: from the index in the root
    /// board, or read in its own board.
    fn record_type(&self, location: &Location) -> Result<Option<RecordType>, IndexError> {
        let Object::Blob(blob) = location.object else {
            return Ok(None);
        };
        let root = location.board.repository == self.repository.path()
            && location.board.source == Source::WorkingTree;
        if !root {
            return Ok(read_type(location, blob));
        }
        let Some(role) = Role::of(&location.path) else {
            return Ok(None);
        };
        let Some(fact) = self.writer.blob(blob, role)? else {
            return Ok(None);
        };
        Ok(match (fact.code, fact.kind) {
            (Some(_), _) => None,
            (None, Kind::Pointer) => POINTER_TYPE.parse().ok(),
            (None, _) => fact.record_type,
        })
    }
}

/// The `$type` of the record at `location`, read in its own board.
fn read_type(location: &Location, blob: ObjectId) -> Option<RecordType> {
    let repository = Repository::open(&location.board.repository).ok()?;
    let bytes = repository
        .read(&location.board.source, &location.path, blob)
        .ok()?;
    match classify(&location.path, bytes.as_deref()) {
        Classified {
            content: Content::Record { record_type, .. },
            code: None,
        } => Some(record_type),
        Classified {
            content: Content::Pointer { .. },
            code: None,
        } => POINTER_TYPE.parse().ok(),
        _ => None,
    }
}

/// Whether an entry at `logical` is at or beneath `scope`.
fn within(logical: Option<&LogicalPath>, scope: Option<&LogicalPath>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    let Some(logical) = logical else {
        return false;
    };
    let mut segments = logical.segments();
    scope
        .segments()
        .all(|segment| segments.next() == Some(segment))
}

/// A check that could not run.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("cannot check {source}")]
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
}

#[cfg(test)]
mod tests;

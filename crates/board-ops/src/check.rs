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
use board_tree::{Entry, Repository, ResolveError, Status, in_scope};

use crate::board::Board;
use crate::index::{Indexed, RefreshError, Refreshed, schema_of, schema_path};

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
    pub change: Change,
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
        Ok(self.decide(index, scope)?.0)
    }

    /// Checks as [`Board::check_in`] does, and also returns every reference
    /// each checked record makes and where it led.
    pub(crate) fn decide(
        &self,
        index: &mut Index,
        scope: Option<&LogicalPath>,
    ) -> Result<(CheckReport, Vec<Referenced>), CheckError> {
        if let Some(scope) = scope {
            self.repository()
                .resolve(&Source::WorkingTree, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let writer = index.write()?;
        let refresh = self.refresh(&writer)?;
        let mut work = refresh.work;
        let mounts = self.mounts(&writer, &refresh, &mut work)?;
        let scopes = mounts.scopes(scope);
        let stats: HashMap<&str, (Stat, ObjectId)> = refresh
            .entries
            .iter()
            .filter_map(|entry| Some((entry.path.as_str(), (entry.stat?, entry.blob?))))
            .collect();
        // A file this refresh saw as it is now need not be hashed again.
        let hashed = Cell::new(work.hashed);
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
        let mut resolver = self
            .repository()
            .resolver(&mut known)
            .map_err(RefreshError::from)?;
        let mut decided = Decided::default();
        let root = Decide {
            writer: &writer,
            facts: &refresh.facts,
            resolve: RefCell::new(Box::new(|path: &LogicalPath| resolver.resolve(path))),
        };
        for entry in &refresh.entries {
            if !in_scope(&scopes, &Source::WorkingTree, entry.logical.as_ref()) {
                continue;
            }
            let change = if refresh.head.get(&entry.path) == Some(&entry.blob) {
                Change::Committed
            } else {
                Change::Uncommitted
            };
            decided.push(
                &root,
                &Source::WorkingTree,
                entry,
                entry.logical.clone(),
                change,
            )?;
        }
        drop(root);
        for mounted in &mounts.sources {
            let source = Source::Commit(mounted.commit);
            let decide = Decide {
                writer: &writer,
                facts: &mounted.facts,
                resolve: RefCell::new(Box::new(|path: &LogicalPath| {
                    mounted.repository.resolve(&source, path)
                })),
            };
            for entry in &mounted.entries {
                if !in_scope(&scopes, &source, entry.logical.as_ref()) {
                    continue;
                }
                let logical = entry
                    .logical
                    .as_ref()
                    .and_then(|logical| mounted.display(logical));
                decided.push(&decide, &source, entry, logical, Change::Committed)?;
            }
        }
        crate::mounts::record(&mounts, &writer, self.repository())?;
        writer.resolve(&decided.resolved)?;
        writer.commit()?;
        let memory = match index.storage() {
            Storage::Memory(fallback) => Some(*fallback),
            Storage::File(_) => None,
        };
        let report = CheckReport {
            entries: decided.checked,
            memory,
            work: Refreshed {
                hashed: hashed.get(),
                ..work
            },
        };
        Ok((report, decided.references))
    }
}

/// What a check decided: the report's entries and the rows of `resolved`,
/// built together so they cannot disagree.
#[derive(Default)]
struct Decided {
    checked: Vec<Checked>,
    resolved: Vec<Resolved>,
    references: Vec<Referenced>,
}

/// One reference a record makes, and where it led this run.
#[derive(Clone, Debug)]
pub(crate) struct Reached {
    pub(crate) pointer: String,
    pub(crate) target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub(crate) location: Option<Location>,
    pub(crate) code: Option<Code>,
}

/// A reference, with the record that makes it.
#[derive(Clone, Debug)]
pub(crate) struct Referenced {
    pub(crate) source: Source,
    pub(crate) path: String,
    pub(crate) logical: Option<LogicalPath>,
    pub(crate) reached: Reached,
}

impl Decided {
    /// Decides `entry` of `source`, named `logical` from the root.
    fn push(
        &mut self,
        decide: &Decide<'_>,
        source: &Source,
        entry: &Entry,
        logical: Option<LogicalPath>,
        change: Change,
    ) -> Result<(), IndexError> {
        let fact = decide.facts.get(&entry.path);
        let role = Role::of(&entry.path);
        let kind = match (role, fact) {
            (None, _) => Kind::File,
            (Some(_), Some(fact)) => fact.fact.kind,
            (Some(_), None) => Kind::Json,
        };
        let code = match entry.status {
            Status::Degraded(code) => Some(code),
            // Only a path too long under its mount point fails here.
            Status::Ok if logical.is_none() => Some(Code::PathMalformed),
            Status::Ok => {
                let mut reached = Vec::new();
                let code = decide.code(entry, &mut reached, role, fact)?;
                self.references
                    .extend(reached.into_iter().map(|reached| Referenced {
                        source: source.clone(),
                        path: entry.path.clone(),
                        logical: logical.clone(),
                        reached,
                    }));
                code
            }
        };
        let schema = fact
            .and_then(schema_path)
            .and_then(|path| schema_of(decide.facts, &path));
        self.resolved.push(Resolved {
            source: source.clone(),
            path: entry.path.clone(),
            logical: logical.clone(),
            blob: entry.blob,
            size: entry.size,
            role,
            schema,
            kind,
            code,
            change,
        });
        self.checked.push(Checked {
            source: source.clone(),
            path: entry.path.clone(),
            logical,
            kind,
            status: code.map_or(Status::Ok, Status::Degraded),
            change,
        });
        Ok(())
    }
}

/// Resolves a path in the source being checked.
type Resolve<'a> = Box<dyn FnMut(&LogicalPath) -> Result<Location, ResolveError> + 'a>;

/// What deciding the status of a source's entries reads.
struct Decide<'a> {
    writer: &'a Writer<'a>,
    /// What the index holds about the source's entries, by physical path.
    facts: &'a HashMap<String, Indexed>,
    resolve: RefCell<Resolve<'a>>,
}

impl Decide<'_> {
    /// The code of an entry whose path is fine.
    fn code(
        &self,
        entry: &Entry,
        reached: &mut Vec<Reached>,
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
            Kind::Record => self.record(fact, reached),
            Kind::Schema | Kind::Json | Kind::File => Ok(None),
        }
    }

    /// Checks a record against its schema and its references.
    fn record(
        &self,
        record: &Indexed,
        reached: &mut Vec<Reached>,
    ) -> Result<Option<Code>, IndexError> {
        let Some(schema_path) = schema_path(record) else {
            return Ok(None);
        };
        let Some(schema) = schema_of(self.facts, &schema_path) else {
            return Ok(Some(match self.facts.get(&schema_path) {
                Some(found) if found.role == Role::Schema => Code::SchemaInvalid,
                _ => Code::RecordUnknownType,
            }));
        };
        let references = match self.writer.validation(record.blob, schema)? {
            None => return Ok(Some(Code::RepoUnreadable)),
            Some(ValidationFact::Invalid { code, .. }) => return Ok(Some(code)),
            Some(ValidationFact::Valid(references)) => references,
        };
        // Every reference is followed, not just those before the first
        // failure, so `refs` can report each one.
        let mut first = None;
        for reference in references {
            let (location, code) = match self.resolve(&reference.target) {
                Err(_) => (None, Some(Code::RefDangling)),
                Ok(location) => {
                    let code = match &reference.accepts {
                        None => None,
                        Some(accepts) => match self.record_type(&location)? {
                            Some(found) if accepts.contains(&found) => None,
                            _ => Some(Code::RefWrongType),
                        },
                    };
                    (Some(location), code)
                }
            };
            first = first.or(code);
            reached.push(Reached {
                pointer: reference.pointer,
                target: reference.target,
                location,
                code,
            });
        }
        Ok(first)
    }

    fn resolve(&self, path: &LogicalPath) -> Result<Location, ResolveError> {
        (self.resolve.borrow_mut())(path)
    }

    /// The `$type` of the record at `location`: from the index when it
    /// holds the blob, which is a fact about content wherever it is, or read
    /// in its own board.
    fn record_type(&self, location: &Location) -> Result<Option<RecordType>, IndexError> {
        let Object::Blob(blob) = location.object else {
            return Ok(None);
        };
        let Some(role) = Role::of(&location.path) else {
            return Ok(None);
        };
        let Some(fact) = self.writer.blob(blob, role)? else {
            return Ok(read_type(location, blob));
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

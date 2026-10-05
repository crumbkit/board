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
    Change, Classified, Code, Content, Fields, Kind, Location, LogicalPath, Object, ObjectId,
    POINTER_TYPE, RecordType, Role, Source, Stat, classify,
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
pub(crate) type Judged = (Option<Location>, Option<(Code, String)>);

/// Why a reference to `target` fails, if it does: it leads nowhere, or,
/// when it `accepts` only some record types, to anything else. `type_of`
/// gives the record type at a location.
pub(crate) fn judge<E>(
    target: &LogicalPath,
    accepts: Option<&[RecordType]>,
    resolved: Result<Location, ResolveError>,
    type_of: impl FnOnce(&Location) -> Result<Option<RecordType>, E>,
) -> Result<Judged, E> {
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
        let wanted = fact.and_then(schema_path);
        let schema = wanted
            .as_ref()
            .and_then(|path| schema_of(decide.facts, path));
        let failure = match entry.status {
            Status::Degraded(code) => Some((code, None)),
            // Only a path too long under its mount point fails here.
            Status::Ok if logical.is_none() => Some((Code::PathMalformed, None)),
            Status::Ok => {
                let mut reached = Vec::new();
                let failure =
                    decide.code(entry, &mut reached, role, fact, (wanted.as_deref(), schema))?;
                self.references
                    .extend(reached.into_iter().map(|reached| Referenced {
                        source: source.clone(),
                        path: entry.path.clone(),
                        logical: logical.clone(),
                        reached,
                    }));
                failure
            }
        };
        let (code, detail) = failure.map_or((None, None), |(code, detail)| (Some(code), detail));
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
            detail,
            change,
            schema: wanted.filter(|_| code == Some(Code::RecordUnknownType)),
        });
        Ok(())
    }
}

/// The schema file a record's type names, from its source's root, and the
/// blob of the usable schema there.
type Wanted<'a> = (Option<&'a str>, Option<ObjectId>);

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
    /// The failure of an entry whose path is fine.
    fn code(
        &self,
        entry: &Entry,
        reached: &mut Vec<Reached>,
        role: Option<Role>,
        fact: Option<&Indexed>,
        schema: Wanted<'_>,
    ) -> Result<Option<Failure>, IndexError> {
        if role.is_none() {
            return Ok(None);
        }
        let Some(fact) = fact else {
            return Ok(Some((Code::RepoUnreadable, None)));
        };
        if let Some(code) = fact.fact.code {
            return Ok(Some((code, None)));
        }
        match fact.fact.kind {
            Kind::Pointer => Ok(entry
                .logical
                .as_ref()
                .and_then(|logical| self.resolve(logical).err())
                .map(|error| {
                    let (code, detail) = unresolved(&error);
                    (code, Some(detail))
                })),
            Kind::Record => self.record(fact, schema, reached),
            Kind::Schema | Kind::Json | Kind::File => Ok(None),
        }
    }

    /// Checks a record against its schema and its references.
    fn record(
        &self,
        record: &Indexed,
        (wanted, schema): Wanted<'_>,
        reached: &mut Vec<Reached>,
    ) -> Result<Option<Failure>, IndexError> {
        let Some(wanted) = wanted else {
            return Ok(None);
        };
        let Some(schema) = schema else {
            // Anything at a schema path is a schema, so it is there but degraded.
            let (code, pointer, reason) = match self.facts.get(wanted) {
                Some(_) => (
                    Code::SchemaInvalid,
                    Some("/$type".to_owned()),
                    format!("the schema at {wanted} is degraded"),
                ),
                None => (
                    Code::RecordUnknownType,
                    None,
                    format!("add its schema at {wanted}"),
                ),
            };
            let detail = Detail {
                pointer,
                reason,
                failures: 1,
            };
            return Ok(Some((code, Some(detail))));
        };
        let references = match self.writer.validation(record.blob, schema)? {
            None => return Ok(Some((Code::RepoUnreadable, None))),
            Some(ValidationFact::Invalid {
                code,
                pointer,
                reason,
                failures,
            }) => {
                let pointer = Some(pointer);
                let detail = Detail {
                    pointer,
                    reason,
                    failures,
                };
                return Ok(Some((code, Some(detail))));
            }
            Some(ValidationFact::Valid(references)) => references,
        };
        // Every reference is followed, not just those before the first
        // failure, so `refs` can report each one.
        let mut tally = Tally::default();
        for reference in references {
            let (location, failure) = judge(
                &reference.target,
                reference.accepts.as_deref(),
                self.resolve(&reference.target),
                |location| self.record_type(location),
            )?;
            tally.add(&reference.pointer, failure.as_ref());
            reached.push(Reached {
                pointer: reference.pointer,
                target: reference.target,
                location,
                code: failure.map(|(code, _)| code),
            });
        }
        Ok(tally.first().map(|(code, detail)| (code, Some(detail))))
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

/// A pointer that does not resolve, for the reason `error` gives.
pub(crate) fn unresolved(error: &ResolveError) -> (Code, Detail) {
    let detail = Detail {
        pointer: None,
        reason: error.to_string(),
        failures: 1,
    };
    (error.code(), detail)
}

/// The `$type` of the record at `location`, read in its own board.
pub(crate) fn read_type(location: &Location, blob: ObjectId) -> Option<RecordType> {
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

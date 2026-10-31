//! `board put`: checking a record and writing it in canonical form, or
//! writing a file as given (docs/architecture.md, Writes).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::convert::Infallible;
use std::time::Duration;

use board_model::{
    BINARY_NAME, Classified, Code, Fields, Found, FrontmatterError, JsonError, Lined, Location,
    LogicalPath, MAX_PATH_BYTES, MAX_RECORD_BYTES, Object, ObjectId, PathError, RecordType,
    Refusal, Role, canonical_json, classify, parse_json,
};
use board_tree::{
    Directory, GitError, LOCK_WAIT, Lock, PinError, Reader, ResolveError, Resolver, Working,
    WriteError, latest,
};
use board_validate::Schema;

use crate::board::Board;
use crate::check::{Detail, Tally, follow, unresolved};
use crate::effects::{Change, EffectError, Shape};
use crate::snapshot::read_type;
use crate::types::TypeError;
use crate::ui::fitted;
use crate::verdict::{self, Admitted, Fault, SchemaFound};

/// What a put expects to find where it writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// Anything: the put overwrites.
    Any,
    /// No file: the put creates one.
    Absent,
    /// A file holding this blob.
    Blob(ObjectId),
}

/// One file a put wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    /// Where the record now is, with every pointer followed to reach it.
    pub location: Location,
    pub display: LogicalPath,
    /// The blob the file held before, or `None` for a new file.
    pub previous: Option<ObjectId>,
    pub blob: ObjectId,
    /// What `check` finds wrong with the written record: a pointer or
    /// reference that does not resolve, or reaches the wrong type, or a
    /// binding that does not fit its view.
    pub degraded: Option<(Code, Detail)>,
}

/// One file a put removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removed {
    pub path: LogicalPath,
    /// The blob the file held.
    pub blob: ObjectId,
}

/// What a put wrote and removed, and the commit it made.
#[derive(Clone, Debug, PartialEq)]
pub struct PutReport {
    pub writes: Vec<Written>,
    pub removed: Vec<Removed>,
    /// The paths a removal that ignores what names nothing found absent.
    pub absent: Vec<LogicalPath>,
    pub commit: Option<ObjectId>,
}

impl PutReport {
    /// What a put wrote, and its commit.
    pub(crate) const fn new(writes: Vec<Written>, commit: Option<ObjectId>) -> Self {
        Self {
            writes,
            removed: Vec::new(),
            absent: Vec::new(),
            commit,
        }
    }

    /// A removal that found `path` absent and removed nothing.
    pub(crate) fn absent(path: LogicalPath) -> Self {
        Self {
            absent: vec![path],
            ..Self::new(Vec::new(), None)
        }
    }
}

/// A record checked and placed, or a file placed, ready to be written.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub(crate) path: LogicalPath,
    pub(crate) expect: Expect,
    /// The record in canonical form, or the file's bytes as given.
    pub(crate) canonical: Vec<u8>,
    pub(crate) target: board_tree::Target,
    pub(crate) admitted: Admitted,
}

/// A working-tree file a put removes, only if it still holds `blob`.
#[derive(Debug)]
pub(crate) struct Removal {
    pub(crate) path: LogicalPath,
    /// The `/`-separated path in the working tree.
    pub(crate) file: String,
    pub(crate) blob: ObjectId,
}

impl Board {
    /// Writes each prepared file, then removes each of `removed`.
    fn apply(
        repository: &board_tree::Repository,
        lock: &Lock,
        prepared: &[Prepared],
        removed: &[Removal],
    ) -> Result<(), WriteError> {
        for item in prepared {
            repository.write_file(lock, &item.target.path, &item.canonical)?;
        }
        for removal in removed {
            repository.store_working(lock, &removal.file)?;
            repository.remove_file(lock, &removal.file)?;
        }
        Ok(())
    }
}

impl Removal {
    /// `Stale` unless the file still holds the blob it held when checked.
    fn stale(&self, repository: &board_tree::Repository) -> Option<PutError> {
        let found = repository.working_blob(&self.file).ok();
        (found != Some(self.blob)).then(|| PutError::Stale {
            path: self.path.clone(),
            found,
        })
    }

    fn removed(&self) -> Removed {
        Removed {
            path: self.path.clone(),
            blob: self.blob,
        }
    }
}

/// Each record type's schema, or what is there instead, read and compiled
/// once while the lock is held, so a batch compiles a schema once per type
/// and not once per line.
#[derive(Debug, Default)]
pub(crate) struct Schemas<'r> {
    held: HashMap<RecordType, Result<Schema, SchemaFound>>,
    /// How many schemas were compiled. Work-count tests assert it.
    pub(crate) compiled: usize,
    /// The working tree's reader, made by the first read, so the schemas
    /// a command reads through symlinks share one snapshot.
    reader: Option<Reader<'r>>,
}

impl<'r> Schemas<'r> {
    /// The reader `reader` holds, made for `board` the first time. It
    /// takes the field, so a caller can borrow the other fields beside it.
    fn reader<'s>(reader: &'s mut Option<Reader<'r>>, board: &'r Board) -> &'s mut Reader<'r> {
        reader.get_or_insert_with(|| board.repository().reader())
    }
}

impl Board {
    /// Takes the lock, waiting up to [`LOCK_WAIT`], checks `bytes` as a
    /// record, and writes it in canonical form to the file `path` names,
    /// only if the file is what `expect` says.
    pub fn put(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<PutReport, PutError> {
        self.put_waiting(path, expect, bytes, LOCK_WAIT)
    }

    /// [`Board::put`], waiting up to `wait` for the lock, and for a mounted
    /// board's lock.
    pub fn put_waiting(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(wait)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let prepared = self.prepare(
            &lock,
            &mut view,
            &mut Schemas::default(),
            path,
            expect,
            bytes,
        );
        let prepared = match prepared {
            // `inside` is only set for one mount, so this is `Through::Mounted`.
            Err(PutError::Write(WriteError::ThroughMount {
                mount,
                inside: Some(inside),
                ..
            })) => {
                let crossing = Crossing {
                    number: 1,
                    path: path.clone(),
                    mount,
                    inside: *inside,
                };
                return self.put_through(lock, &[crossing], Sent::Record(expect, bytes), wait);
            }
            prepared => prepared?,
        };
        let message = format!("{BINARY_NAME} put {path}");
        self.write_all(lock, &[prepared], &[], &message)
            .map_err(Failure::one)
    }

    /// Checks one record and finds where it lands in `view`, under the lock
    /// that the write will hold, so that what is checked is what is written.
    pub(crate) fn prepare<'b>(
        &'b self,
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        schemas: &mut Schemas<'b>,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<Prepared, PutError> {
        let _timed = self.timed("put.validate");
        let admitted = self.check_record(schemas, bytes);
        let pointer = admitted.as_ref().is_ok_and(|admitted| admitted.pointer);
        // A record through a mount is checked against the mounted board's
        // schemas, so the mount is named before this board's verdict.
        let target = match view.target(path, pointer) {
            Err(error @ WriteError::ThroughMount { .. }) => return Err(error.into()),
            target => target,
        };
        let refuse = |refused| PutError::Record {
            path: path.clone(),
            refused,
        };
        let admitted = admitted.map_err(refuse)?;
        let target = target?;
        let canonical = canonical_json(&admitted.value).into_bytes();
        let change = Change::Put(&canonical, &admitted, Shape::Record);
        self.gate(&target, change)?;
        Ok(Prepared {
            path: path.clone(),
            expect,
            canonical,
            target,
            admitted,
        })
    }

    /// Compares every expect under `lock`, the lock the records were
    /// prepared under, and writes every prepared record in order, then
    /// removes each of `removed`, with one commit when `commit_on_put` is
    /// set. Writes nothing unless every expect holds and every file to
    /// remove holds its blob.
    pub(crate) fn write_all(
        &self,
        lock: Lock,
        prepared: &[Prepared],
        removed: &[Removal],
        message: &str,
    ) -> Result<PutReport, Failure> {
        let _timed = self.timed("put.write");
        let repository = self.repository();
        let mut writes = Vec::with_capacity(prepared.len());
        let mut stale = Vec::new();
        for (at, item) in prepared.iter().enumerate() {
            let found = repository
                .read_working(&item.target.path)
                .map_err(|error| Failure::Error(WriteError::from(error).into()))?;
            let previous = match &found {
                Working::Missing => None,
                Working::Read(contents) => Some(contents.blob),
                Working::TooLarge => Some(
                    repository
                        .working_blob(&item.target.path)
                        .map_err(|error| Failure::Error(WriteError::from(error).into()))?,
                ),
            };
            let matches = match item.expect {
                Expect::Any => true,
                Expect::Absent => found == Working::Missing,
                Expect::Blob(blob) => previous == Some(blob),
            };
            if !matches {
                stale.push((
                    at,
                    PutError::Stale {
                        path: item.path.clone(),
                        found: previous,
                    },
                ));
            }
            let blob = repository.blob_id(&item.canonical);
            let location = Location {
                path: item.target.path.clone(),
                object: Object::Blob(blob),
                ..item.target.location.clone()
            };
            writes.push(Written {
                display: location.display_path().unwrap_or_else(|| item.path.clone()),
                location,
                previous,
                blob,
                degraded: None,
            });
        }
        stale.extend(
            removed.iter().enumerate().filter_map(|(at, removal)| {
                Some((prepared.len() + at, removal.stale(repository)?))
            }),
        );
        if !stale.is_empty() {
            return Err(Failure::Stale(stale));
        }
        // The commit is made ready before anything is renamed, so a put
        // with no identity to commit as writes nothing.
        let staged = if self.config().commit_on_put {
            let files: Vec<_> = (prepared.iter())
                .map(|item| (item.target.path.as_str(), Some(item.canonical.as_slice())))
                .chain(removed.iter().map(|removal| (removal.file.as_str(), None)))
                .collect();
            repository
                .stage_commit(&lock, &files, message)
                .map_err(|error| Failure::Error(error.into()))?
        } else {
            None
        };
        Self::apply(repository, &lock, prepared, removed)
            .map_err(|error| Failure::Error(error.into()))?;
        let commit = match staged {
            None => None,
            Some(staged) => match repository.finish_commit(&lock, staged) {
                Ok(commit) => Some(commit),
                Err(WriteError::HeadMoved) => {
                    return Err(Failure::Error(PutError::HeadMoved { writes }));
                }
                Err(error) => return Err(Failure::Error(error.into())),
            },
        };
        drop(lock);
        let mut view = self.repositories().snapshot().map_err(ResolveError::Git);
        let root = &mut self.repositories().root().reader();
        for (written, item) in writes.iter_mut().zip(prepared) {
            written.degraded = match &mut view {
                Ok(view) => {
                    let resolve = &mut |path: &LogicalPath| view.resolve(path);
                    self.degraded((resolve, root), item, &written.display)
                }
                Err(error) => Self::unresolvable(item, error),
            };
        }
        let removed = removed.iter().map(Removal::removed).collect();
        Ok(PutReport {
            removed,
            ..PutReport::new(writes, commit)
        })
    }

    /// Checks `bytes` as `check` checks a record, with its codes, and
    /// returns what it admits. Input that is not one JSON object with a
    /// `$type` is `put::invalid`.
    pub(crate) fn check_record<'b>(
        &'b self,
        schemas: &mut Schemas<'b>,
        bytes: &[u8],
    ) -> Result<Admitted, Refused> {
        Ok(self.admit(schemas, checked(bytes)?)?)
    }

    /// Judges `classified` as a record, against the schema of its type that
    /// `schemas` reads once.
    pub(crate) fn admit<'b>(
        &'b self,
        schemas: &mut Schemas<'b>,
        classified: Classified,
    ) -> Result<Admitted, Fault> {
        let find = move |record_type: &RecordType| {
            // Moving the borrow out makes this `FnOnce`, so it can lend the schema.
            let Schemas {
                held,
                compiled,
                reader,
                ..
            } = { schemas };
            let schema = match held.entry(record_type.clone()) {
                Entry::Occupied(held) => held.into_mut(),
                Entry::Vacant(entry) => {
                    let reader = Schemas::reader(reader, self);
                    let schema = self.read_schema(reader, entry.key());
                    *compiled += usize::from(schema.is_ok());
                    entry.insert(schema)
                }
            };
            schema.as_ref().map_err(SchemaFound::clone)
        };
        verdict::record(classified, find)
    }

    /// Reads and compiles the schema of `record_type` from the working tree.
    fn read_schema(
        &self,
        reader: &mut Reader<'_>,
        record_type: &RecordType,
    ) -> Result<Schema, SchemaFound> {
        let schema_path = record_type.schema_path();
        match self.repository().working_directory(&schema_path) {
            Ok(None) => {}
            Ok(Some(Directory::Entries | Directory::Ignored)) => return Err(SchemaFound::Missing),
            Ok(Some(Directory::Submodule)) | Err(_) => return Err(SchemaFound::Unreadable),
        }
        let schema_bytes = match reader.read_through(&schema_path) {
            Ok((Working::Read(contents), _)) => Some(contents.bytes),
            Ok((Working::TooLarge, _)) => None,
            Ok((Working::Missing, _)) => return Err(SchemaFound::Missing),
            Err(GitError::Symlink { refused, .. }) if refused.code != Code::RepoUnreadable => {
                return Err(SchemaFound::Symlink(refused));
            }
            Err(_) => return Err(SchemaFound::Unreadable),
        };
        verdict::judge(&classify(&schema_path, schema_bytes.as_deref()))
            .ok()
            .flatten()
            .ok_or(SchemaFound::Unusable)
    }

    /// What `check` finds wrong with `item`, just written at `display`,
    /// with `resolve` and `root`, the root repository's reader, reading the
    /// working tree as written.
    fn degraded(
        &self,
        (resolve, root): (
            &mut impl FnMut(&LogicalPath) -> Result<Location, ResolveError>,
            &mut Reader<'_>,
        ),
        item: &Prepared,
        display: &LogicalPath,
    ) -> Option<(Code, Detail)> {
        if item.admitted.pointer {
            return resolve(display).err().map(|error| unresolved(&error));
        }
        let mut tally = Tally::default();
        for reference in &item.admitted.references {
            let Ok((_, failure)) = follow::<Infallible>(
                &reference.path,
                reference.accepts.as_deref(),
                resolve(&reference.path),
                |location| match location.object {
                    Object::Blob(blob) => Ok(read_type(self.repositories(), root, location, blob)),
                    Object::Directory => Ok(None),
                },
            );
            tally.add(&reference.pointer, failure.as_ref());
        }
        let (binding, repositories) = (item.admitted.binding(), self.repositories());
        let read = &mut |location: &Location, blob| latest(repositories.read(root, location, blob));
        (tally.first()).or_else(|| fitted((display, Some(binding?)), read, resolve))
    }

    /// What `check` finds wrong with `item` when nothing can be resolved
    /// because of `error`: its pointer, or each of its references, fails.
    pub(crate) fn unresolvable(item: &Prepared, error: &ResolveError) -> Option<(Code, Detail)> {
        if item.admitted.pointer {
            return Some(unresolved(error));
        }
        let dangling = (Code::RefDangling, error.to_string());
        let mut tally = Tally::default();
        for reference in &item.admitted.references {
            tally.add(&reference.pointer, Some(&dangling));
        }
        tally.first()
    }
}

/// Why a record is refused, with what failed when Board knows more than the
/// code says.
#[derive(Debug, thiserror::Error)]
pub enum Refused {
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    Fault(#[from] Fault),
    #[error("it is empty")]
    Empty,
    #[error("it is not a json object")]
    NotAnObject,
    #[error("it has no $type")]
    NoType,
}

impl Refused {
    pub fn code(&self) -> Code {
        match self {
            Self::Json(error) => error.code(),
            Self::Frontmatter(error) => error.code(),
            Self::Fault(fault) => fault.code(),
            Self::Empty => Code::JsonParse,
            Self::NotAnObject | Self::NoType => Code::PutInvalid,
        }
    }

    /// The type and schema file of a record with no usable schema, or the
    /// pointer a schema refused.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Fault(fault) => fault.fields(),
            Self::Json(_)
            | Self::Frontmatter(_)
            | Self::Empty
            | Self::NotAnObject
            | Self::NoType => Fields::default(),
        }
    }
}

/// Why `write_all` wrote nothing: stale expects, by their index among the
/// prepared records, or any other error.
#[derive(Debug)]
pub(crate) enum Failure {
    Stale(Vec<(usize, PutError)>),
    Error(PutError),
}

impl Failure {
    /// The error of a put of one record or file.
    pub(crate) fn one(self) -> PutError {
        match self {
            Self::Stale(mut stale) => stale.remove(0).1,
            Self::Error(error) => error,
        }
    }
}

/// A put that wrote nothing, or for `head_moved`, wrote but did not commit.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("the record for {path} {}", said(refused))]
    Record { path: LogicalPath, refused: Refused },
    /// A record refused before `put --new` found its folder.
    #[error("the record {}", said(refused))]
    Unplaced { refused: Refused },
    #[error("{path} is not what --expect says: it holds {}", found.map_or_else(|| "nothing".to_owned(), |blob| blob.to_string()))]
    Stale {
        path: LogicalPath,
        found: Option<ObjectId>,
    },
    #[error(
        "HEAD moved while the put ran, so {} written but not committed",
        writes.iter().map(|written| written.display.to_string()).collect::<Vec<_>>().join(", ") + if writes.len() == 1 { " is" } else { " are" }
    )]
    HeadMoved { writes: Vec<Written> },
    #[error("the batch is refused")]
    Batch { code: Code, refusals: Vec<Lined> },
    #[error("{reason}")]
    Line { code: Code, reason: String },
    #[error("a batch holds more than {MAX_BATCH_LINES} lines")]
    TooManyLines,
    #[error("{path} cannot be written: {bytes} bytes is over {MAX_RECORD_BYTES}")]
    TooLarge { path: LogicalPath, bytes: usize },
    #[error("{path} cannot be written: its text holds NUL at byte offset {offset}")]
    Nul { path: LogicalPath, offset: usize },
    #[error("the batch cannot be read: {source}")]
    Input { source: std::io::Error },
    #[error("{path} cannot be uploaded to {location}: {source}")]
    Upload {
        path: LogicalPath,
        location: String,
        source: std::io::Error,
    },
    #[error("--new takes a folder, and {folder} does not end in /")]
    NotAFolder { folder: LogicalPath },
    #[error("a new name under {folder} would take its path over {MAX_PATH_BYTES} bytes")]
    TooDeep { folder: LogicalPath },
    #[error("every name drawn under {folder} is taken, after {tries} tries")]
    NameTaken { folder: LogicalPath, tries: usize },
    #[error("{record_type} declares no folder, so `put --new` needs one")]
    NoFolder { record_type: RecordType },
    #[error("{record_type} files records under {folder}, and no --var gives {{{var}}}")]
    FolderUnfilled {
        record_type: RecordType,
        folder: String,
        var: String,
    },
    #[error("--var {var} names no placeholder of the folder {folder}")]
    VarUnused { var: String, folder: String },
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error(transparent)]
    Mount(#[from] PinError),
    /// A schema line of a batch refused as `type add` refuses it.
    #[error(transparent)]
    Type(Box<TypeError>),
    #[error(transparent)]
    Effect(Box<EffectError>),
}

impl From<EffectError> for PutError {
    fn from(error: EffectError) -> Self {
        Self::Effect(Box::new(error))
    }
}

impl PutError {
    pub fn code(&self) -> Code {
        match self {
            Self::Stale { .. } => Code::PutStale,
            Self::HeadMoved { .. } => Code::PutHeadMoved,
            Self::Record { refused, .. } | Self::Unplaced { refused } => refused.code(),
            Self::Batch { code, .. } | Self::Line { code, .. } => *code,
            Self::TooManyLines => Code::PutInvalid,
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::Nul { .. } => Code::PutNul,
            Self::Input { .. } => Code::RepoUnreadable,
            Self::Upload { .. } => Code::PutFailed,
            Self::NotAFolder { .. } => Code::PutNotAFolder,
            Self::NameTaken { .. } => Code::PutNameTaken,
            Self::TooDeep { .. } => Code::PathMalformed,
            Self::NoFolder { .. } => Code::PutNoFolder,
            Self::FolderUnfilled { .. } => Code::PutFolderUnfilled,
            Self::VarUnused { .. } => Code::PutVarUnused,
            Self::Path(error) => error.code(),
            Self::Write(error) => error.code(),
            Self::Mount(error) => error.code(),
            Self::Type(error) => error.code(),
            Self::Effect(error) => error.code(),
        }
    }

    /// The path and what is wrong at it; for a stale put, what the path
    /// holds; for a batch, each line it refuses.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Record { path, refused } => Fields {
                path: Some(path.clone()),
                ..refused.fields()
            },
            Self::Unplaced { refused } => refused.fields(),
            Self::NoFolder { record_type } => Fields {
                record_type: Some(record_type.clone()),
                ..Fields::default()
            },
            Self::FolderUnfilled {
                record_type, var, ..
            } => Fields {
                record_type: Some(record_type.clone()),
                var: Some(var.clone()),
                ..Fields::default()
            },
            Self::VarUnused { var, .. } => Fields {
                var: Some(var.clone()),
                ..Fields::default()
            },
            Self::Stale { path, found } => Fields {
                blob: *found,
                found: Some(found.map_or(Found::Nothing, Found::Blob)),
                ..Fields::at(path)
            },
            Self::Batch { refusals, .. } => Fields {
                refusals: refusals.clone(),
                ..Fields::default()
            },
            Self::TooLarge { path, .. } | Self::Upload { path, .. } => Fields::at(path),
            Self::NotAFolder { folder } | Self::NameTaken { folder, .. } => Fields::at(folder),
            Self::TooDeep { folder } => Fields {
                directory: Some(folder.as_str().trim_start_matches('/').to_owned()),
                ..Fields::at(folder)
            },
            Self::Nul { path, offset } => Fields {
                offset: Some(*offset),
                ..Fields::at(path)
            },
            Self::Write(error) => error.fields(),
            Self::Mount(error) => error.fields(),
            Self::Type(error) => error.fields(),
            Self::Effect(error) => error.fields(),
            Self::HeadMoved { .. }
            | Self::Line { .. }
            | Self::TooManyLines
            | Self::Path(_)
            | Self::Input { .. } => Fields::default(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Effect(error) => error.refusal(),
            _ => Refusal::new(self.code(), self, &self.fields()),
        }
    }
}

/// `bytes` classified as a record, once they are one JSON object with a
/// `$type`; any other input is `put::invalid`.
pub(crate) fn checked(bytes: &[u8]) -> Result<Classified, Refused> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(Refused::Empty);
    }
    let value = parse_json(bytes)?;
    let Some(object) = value.as_object() else {
        return Err(Refused::NotAnObject);
    };
    if !object.contains_key("$type") {
        return Err(Refused::NoType);
    }
    Ok(Role::Json.classify(Some(bytes)))
}

/// What a refused record is, after "the record" or its path.
fn said(refused: &Refused) -> String {
    match refused {
        Refused::Fault(Fault::Code(_)) => "is refused".to_owned(),
        Refused::Fault(Fault::Record(error)) => error.to_string(),
        _ => format!("is refused: {refused}"),
    }
}

mod batch;
mod file;
mod folder;
mod new;
mod remove;
mod through;

pub use batch::MAX_BATCH_LINES;
pub(crate) use batch::{Checked, ReadLine};
pub use folder::Vars;
pub use new::folder_text;
pub use remove::{Removing, Unmatched};
use through::{Crossing, Sent};

#[cfg(test)]
mod tests;

//! `board put`: checking a record and writing it in canonical form
//! (docs/architecture.md, Writes).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::convert::Infallible;
use std::io::{BufRead, Read};

use board_model::{
    BINARY_NAME, CONFIG_TYPE, Code, Content, Fields, JsonError, Location, LogicalPath,
    MANIFEST_TYPE, MAX_RECORD_BYTES, Object, ObjectId, RecordType, Role, canonical_json,
    parse_json,
};
use board_plugin::{Manifest, ManifestError};
use board_tree::{LOCK_WAIT, Lock, ResolveError, Resolver, Working, WriteError};
use board_validate::{RecordError, Reference, Schema};
use serde_json::Value;

use crate::board::{Board, Config};
use crate::check::{Detail, Tally, judge, read_type, unresolved};

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
    /// reference that does not resolve, or reaches the wrong type.
    pub degraded: Option<(Code, Detail)>,
}

/// What a put wrote, and the commit it made.
#[derive(Clone, Debug, PartialEq)]
pub struct PutReport {
    pub writes: Vec<Written>,
    pub commit: Option<ObjectId>,
}

/// A record checked and placed, ready to be written.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub(crate) path: LogicalPath,
    pub(crate) expect: Expect,
    pub(crate) canonical: Vec<u8>,
    pub(crate) target: board_tree::Target,
    pub(crate) admitted: Admitted,
}

/// A record `check_record` admits: its value, and what `check` follows from
/// it once written, its target when it is a pointer or else its references.
#[derive(Debug)]
pub(crate) struct Admitted {
    pub(crate) value: Value,
    pub(crate) pointer: bool,
    pub(crate) references: Vec<Reference>,
}

/// The refusal of line `line` for `error`.
fn refusal(line: usize, error: &PutError) -> Refusal {
    Refusal {
        line,
        code: error.code(),
        message: error.to_string(),
    }
}

/// One line of a batch that refused it, counted from 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub line: usize,
    pub code: Code,
    pub message: String,
}

/// The most lines one batch may hold.
pub const MAX_BATCH_LINES: usize = 10_000;

/// One line of `put --batch`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    path: String,
    #[serde(default)]
    expect: Option<String>,
    record: Value,
}

/// Each record type's schema, read and compiled once while the lock is held,
/// so a batch compiles a schema once per type and not once per line.
#[derive(Debug, Default)]
pub(crate) struct Schemas {
    held: HashMap<RecordType, Schema>,
    /// How many schemas were compiled. Work-count tests assert it.
    pub(crate) compiled: usize,
}

/// A batch line read with its number, or why it is refused.
pub(crate) type ReadLine = Result<(usize, Line), Refusal>;

/// One batch line, read but not yet checked.
pub(crate) struct Line {
    path: LogicalPath,
    expect: Expect,
    bytes: Vec<u8>,
}

impl Board {
    /// Takes the lock, checks `bytes` as a record, and writes it in
    /// canonical form to the file `path` names, only if the file is what
    /// `expect` says.
    pub fn put(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repository().snapshot().map_err(WriteError::from)?;
        let prepared = self.prepare(
            &lock,
            &mut view,
            &mut Schemas::default(),
            path,
            expect,
            bytes,
        )?;
        let message = format!("{BINARY_NAME} put {path}");
        self.write_all(lock, &[prepared], &message)
            .map_err(|failure| match failure {
                Failure::Stale(mut stale) => stale.remove(0).1,
                Failure::Error(error) => error,
            })
    }

    /// Reads JSON Lines of `{path, expect, record}` from `input`, then takes
    /// the lock, checks every line and compares every expect, and writes
    /// them in order only if none refuses, with one commit.
    pub fn put_batch(&self, input: &mut dyn BufRead) -> Result<PutReport, PutError> {
        let lines = Self::read_lines(input)?;
        if lines.is_empty() {
            return Ok(PutReport {
                writes: Vec::new(),
                commit: None,
            });
        }
        if lines.iter().all(Result::is_err) {
            return Err(PutError::Batch {
                code: Code::PutInvalid,
                refusals: lines.into_iter().filter_map(Result::err).collect(),
            });
        }
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repository().snapshot().map_err(WriteError::from)?;
        let (prepared, numbers) =
            self.check_lines(&lock, &mut view, lines, &mut Schemas::default())?;
        let message = format!("{BINARY_NAME} put --batch ({} records)", prepared.len());
        self.write_all(lock, &prepared, &message)
            .map_err(|failure| match failure {
                Failure::Stale(stale) => PutError::Batch {
                    code: Code::PutStale,
                    refusals: stale
                        .into_iter()
                        .map(|(at, error)| refusal(numbers[at], &error))
                        .collect(),
                },
                Failure::Error(error) => error,
            })
    }

    /// Reads every line of a batch, each with its number or why it is
    /// refused.
    pub(crate) fn read_lines(input: &mut dyn BufRead) -> Result<Vec<ReadLine>, PutError> {
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for number in 1.. {
            line.clear();
            let read = input
                .take(MAX_RECORD_BYTES as u64 + 2)
                .read_until(b'\n', &mut line)
                .map_err(|source| PutError::Input { source })?;
            if read == 0 {
                break;
            }
            if number > MAX_BATCH_LINES {
                return Err(PutError::TooManyLines);
            }
            let text = line.strip_suffix(b"\n").unwrap_or(&line);
            if text.len() > MAX_RECORD_BYTES {
                // The rest of the line has not been read, so no later line
                // can be found.
                lines.push(Err(Refusal {
                    line: number,
                    code: Code::JsonTooLarge,
                    message: format!("the line is over {MAX_RECORD_BYTES} bytes"),
                }));
                break;
            }
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            lines.push(match Self::parse_line(text) {
                Ok(parsed) => Ok((number, parsed)),
                Err(error) => Err(refusal(number, &error)),
            });
        }
        Ok(lines)
    }

    /// Checks every line of a batch under `lock` against the one snapshot
    /// `view`, and returns each prepared record with its line number, or
    /// every refusal.
    pub(crate) fn check_lines(
        &self,
        lock: &Lock,
        view: &mut Resolver<'_, '_>,
        lines: Vec<ReadLine>,
        schemas: &mut Schemas,
    ) -> Result<(Vec<Prepared>, Vec<usize>), PutError> {
        let mut prepared: Vec<Prepared> = Vec::new();
        let mut numbers: Vec<usize> = Vec::new();
        let mut refusals: Vec<Refusal> = Vec::new();
        // Each target folded as the walk folds names, so two lines that a
        // case-insensitive file system would make one file clash.
        let mut folded: HashMap<String, usize> = HashMap::new();
        for parsed in lines {
            let checked = parsed.and_then(|(number, line)| {
                match self.prepare(lock, view, schemas, &line.path, line.expect, &line.bytes) {
                    Ok(item) => Ok((number, item)),
                    Err(error) => Err(refusal(number, &error)),
                }
            });
            let (number, item) = match checked {
                Ok(checked) => checked,
                Err(refused) => {
                    refusals.push(refused);
                    continue;
                }
            };
            match folded.entry(item.target.path.to_ascii_lowercase()) {
                Entry::Occupied(first) => {
                    let first = *first.get();
                    let earlier = &prepared[first].target.path;
                    let (code, message) = if *earlier == item.target.path {
                        (Code::PutInvalid, "as")
                    } else {
                        (Code::PathCaseCollision, "differing only in case from")
                    };
                    refusals.push(Refusal {
                        line: number,
                        code,
                        message: format!(
                            "it writes {} {message} line {} writes {earlier}",
                            item.target.path, numbers[first]
                        ),
                    });
                }
                Entry::Vacant(entry) => {
                    entry.insert(prepared.len());
                }
            }
            prepared.push(item);
            numbers.push(number);
        }
        if refusals.is_empty() {
            Ok((prepared, numbers))
        } else {
            Err(PutError::Batch {
                code: Code::PutInvalid,
                refusals,
            })
        }
    }

    /// Reads one batch line.
    fn parse_line(line: &[u8]) -> Result<Line, PutError> {
        let invalid = |reason: String| PutError::Line {
            code: Code::PutInvalid,
            reason: format!("the line is not {{path, expect, record}}: {reason}"),
        };
        let value = parse_json(line).map_err(|error| PutError::Line {
            code: error.code(),
            reason: error.to_string(),
        })?;
        let raw: RawLine =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        let path: LogicalPath = raw
            .path
            .parse()
            .map_err(|_| invalid(format!("{:?} is not a logical path", raw.path)))?;
        let expect = match raw.expect.as_deref() {
            None => Expect::Any,
            Some("none") => Expect::Absent,
            Some(hex) => ObjectId::from_hex(hex)
                .map(Expect::Blob)
                .ok_or_else(|| invalid(format!("expect {hex:?} is not `none` or a blob id")))?,
        };
        let bytes = serde_json::to_vec(&raw.record).map_err(|error| invalid(error.to_string()))?;
        Ok(Line {
            path,
            expect,
            bytes,
        })
    }

    /// Checks one record and finds where it lands in `view`, under the lock
    /// that the write will hold, so that what is checked is what is written.
    pub(crate) fn prepare(
        &self,
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        schemas: &mut Schemas,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<Prepared, PutError> {
        let admitted = self
            .check_record(schemas, bytes)
            .map_err(|refused| PutError::Record {
                path: path.clone(),
                refused,
            })?;
        let target = view.target(path, admitted.pointer)?;
        Ok(Prepared {
            path: path.clone(),
            expect,
            canonical: canonical_json(&admitted.value),
            target,
            admitted,
        })
    }

    /// Compares every expect under `lock`, the lock the records were
    /// prepared under, and writes every prepared record in order, with one
    /// commit when `commit_on_put` is set. Writes nothing unless every
    /// expect holds.
    pub(crate) fn write_all(
        &self,
        lock: Lock,
        prepared: &[Prepared],
        message: &str,
    ) -> Result<PutReport, Failure> {
        let repository = self.repository();
        let mut writes = Vec::with_capacity(prepared.len());
        let mut stale = Vec::new();
        for (at, item) in prepared.iter().enumerate() {
            let found = repository
                .read_working(&item.target.path, None)
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
        if !stale.is_empty() {
            return Err(Failure::Stale(stale));
        }
        // The commit is made ready before anything is renamed, so a put
        // with no identity to commit as writes nothing.
        let staged = if self.config().commit_on_put {
            let files: Vec<(&str, &[u8])> = prepared
                .iter()
                .map(|item| (item.target.path.as_str(), item.canonical.as_slice()))
                .collect();
            Some(
                repository
                    .stage_commit(&lock, &files, message)
                    .map_err(|error| Failure::Error(error.into()))?,
            )
        } else {
            None
        };
        for item in prepared {
            repository
                .write_file(&lock, &item.target.path, &item.canonical)
                .map_err(|error| Failure::Error(error.into()))?;
        }
        let commit = match staged {
            None => None,
            Some(staged) => match repository.finish_commit(staged) {
                Ok(commit) => Some(commit),
                Err(WriteError::HeadMoved) => {
                    return Err(Failure::Error(PutError::HeadMoved { writes }));
                }
                Err(error) => return Err(Failure::Error(error.into())),
            },
        };
        drop(lock);
        let mut view = repository.snapshot().map_err(ResolveError::Git);
        for (written, item) in writes.iter_mut().zip(prepared) {
            written.degraded = match &mut view {
                Ok(view) => Self::degraded(&mut |path| view.resolve(path), item, &written.display),
                Err(error) => Self::unresolvable(item, error),
            };
        }
        Ok(PutReport { writes, commit })
    }

    /// Checks `bytes` as `check` checks a record, with its codes, and
    /// returns what it admits. Input that is not one JSON object with a
    /// `$type` is `put::invalid`.
    pub(crate) fn check_record(
        &self,
        schemas: &mut Schemas,
        bytes: &[u8],
    ) -> Result<Admitted, Refused> {
        let admitted = |value, pointer| Admitted {
            value,
            pointer,
            references: Vec::new(),
        };
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
        let classified = Role::Json.classify(Some(bytes));
        if let Some(code) = classified.code {
            return Err(Refused::Code(code));
        }
        let record_type = match classified.content {
            Content::Pointer { .. } => return Ok(admitted(value, true)),
            Content::Record { record_type, .. } => record_type,
            Content::Json { .. }
            | Content::Schema { .. }
            | Content::File
            | Content::Unparsed(_) => {
                return Err(Refused::Code(Code::PutInvalid));
            }
        };
        if record_type.as_str() == CONFIG_TYPE {
            return Config::from_value(&value)
                .map(|_| admitted(value, false))
                .ok_or(Refused::Code(Code::ConfigInvalid));
        }
        if record_type.as_str() == MANIFEST_TYPE {
            return Manifest::from_value(value.clone())
                .map(|_| admitted(value, false))
                .map_err(Refused::from);
        }
        let schema = match schemas.held.entry(record_type) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(entry) => {
                let schema = self.compile_schema(entry.key())?;
                schemas.compiled += 1;
                entry.insert(schema)
            }
        };
        schema.validate(&value)?;
        let references = schema.references(&value);
        Ok(Admitted {
            references,
            ..admitted(value, false)
        })
    }

    /// Reads and compiles the schema of `record_type` from the working tree.
    fn compile_schema(&self, record_type: &RecordType) -> Result<Schema, Refused> {
        let schema_path = record_type.schema_path();
        let schema_bytes = match self.repository().read_working(&schema_path, None) {
            Ok(Working::Read(contents)) => Some(contents.bytes),
            Ok(Working::TooLarge) => None,
            Ok(Working::Missing) => {
                return Err(Refused::UnknownType {
                    record_type: record_type.clone(),
                    schema: schema_path,
                });
            }
            Err(_) => return Err(Refused::Code(Code::RepoUnreadable)),
        };
        match Role::of(&schema_path).map(|role| role.classify(schema_bytes.as_deref())) {
            Some(board_model::Classified {
                content: Content::Schema { value },
                code: None,
            }) => Schema::compile(value).map_err(|_| Refused::Code(Code::SchemaInvalid)),
            _ => Err(Refused::Code(Code::SchemaInvalid)),
        }
    }

    /// What `check` finds wrong with `item`, just written at `display`,
    /// with `resolve` reading the working tree as written.
    fn degraded(
        resolve: &mut impl FnMut(&LogicalPath) -> Result<Location, ResolveError>,
        item: &Prepared,
        display: &LogicalPath,
    ) -> Option<(Code, Detail)> {
        if item.admitted.pointer {
            return resolve(display).err().map(|error| unresolved(&error));
        }
        let mut tally = Tally::default();
        for reference in &item.admitted.references {
            let Ok((_, failure)) = judge::<Infallible>(
                &reference.path,
                reference.accepts.as_deref(),
                resolve(&reference.path),
                |location| match location.object {
                    Object::Blob(blob) => Ok(read_type(location, blob)),
                    Object::Directory => Ok(None),
                },
            );
            tally.add(&reference.pointer, failure.as_ref());
        }
        tally.first()
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
    Schema(#[from] RecordError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("it is empty")]
    Empty,
    #[error("it is not a json object")]
    NotAnObject,
    #[error("it has no $type")]
    NoType,
    #[error("no schema for {record_type} at {schema}")]
    UnknownType {
        record_type: RecordType,
        schema: String,
    },
    #[error("{0}")]
    Code(Code),
}

impl Refused {
    pub fn code(&self) -> Code {
        match self {
            Self::Json(error) => error.code(),
            Self::Schema(error) => error.code(),
            Self::Manifest(error) => error.code(),
            Self::Empty => Code::JsonParse,
            Self::NotAnObject | Self::NoType => Code::PutInvalid,
            Self::UnknownType { .. } => Code::RecordUnknownType,
            Self::Code(code) => *code,
        }
    }

    /// The type and schema of an unknown type, or the pointer a schema refused.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Schema(error) => error.fields(),
            Self::UnknownType {
                record_type,
                schema,
            } => Fields {
                record_type: Some(record_type.clone()),
                schema: Some(schema.clone()),
                ..Fields::default()
            },
            Self::Json(_)
            | Self::Manifest(_)
            | Self::Empty
            | Self::NotAnObject
            | Self::NoType
            | Self::Code(_) => Fields::default(),
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

/// A put that wrote nothing, or for `head_moved`, wrote but did not commit.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("the record for {path} is refused{}", match refused {
        Refused::Code(_) => String::new(),
        _ => format!(": {refused}"),
    })]
    Record { path: LogicalPath, refused: Refused },
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
    #[error("the batch is refused: {}", refusals.iter().map(|refusal| format!("line {}: {}: {}", refusal.line, refusal.code, refusal.message)).collect::<Vec<_>>().join("; "))]
    Batch { code: Code, refusals: Vec<Refusal> },
    #[error("{reason}")]
    Line { code: Code, reason: String },
    #[error("a batch holds more than {MAX_BATCH_LINES} lines")]
    TooManyLines,
    #[error("the batch cannot be read: {source}")]
    Input { source: std::io::Error },
    #[error(transparent)]
    Write(#[from] WriteError),
}

impl PutError {
    pub fn code(&self) -> Code {
        match self {
            Self::Stale { .. } => Code::PutStale,
            Self::HeadMoved { .. } => Code::PutHeadMoved,
            Self::Record { refused, .. } => refused.code(),
            Self::Batch { code, .. } | Self::Line { code, .. } => *code,
            Self::TooManyLines => Code::PutInvalid,
            Self::Input { .. } => Code::RepoUnreadable,
            Self::Write(error) => error.code(),
        }
    }

    /// The path and what is wrong at it; for a stale put, the blob the path
    /// holds, or none when it holds nothing.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Record { path, refused } => Fields {
                path: Some(path.clone()),
                ..refused.fields()
            },
            Self::Stale { path, found } => Fields {
                blob: *found,
                ..Fields::at(path)
            },
            Self::Write(error) => error.fields(),
            Self::HeadMoved { .. }
            | Self::Batch { .. }
            | Self::Line { .. }
            | Self::TooManyLines
            | Self::Input { .. } => Fields::default(),
        }
    }
}

#[cfg(test)]
mod tests;

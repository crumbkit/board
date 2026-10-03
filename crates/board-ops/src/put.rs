//! `board put`: checking a record and writing it in canonical form
//! (docs/architecture.md, Writes).

use std::io::{BufRead, Read};

use board_model::{
    CONFIG_TYPE, Code, Content, Location, LogicalPath, MAX_RECORD_BYTES, Object, ObjectId, Role,
    canonical_json, parse_json,
};
use board_tree::{LOCK_WAIT, WriteError};
use board_validate::Schema;
use serde_json::Value;

use crate::board::{Board, Config};

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

impl Board {
    /// Checks `bytes` as a record, then writes it in canonical form to the
    /// file `path` names, under the lock and only if the file is what
    /// `expect` says.
    pub fn put(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<PutReport, PutError> {
        let prepared = self.prepare(path, expect, bytes)?;
        let message = format!("board put {path}");
        self.write_all(&[prepared], &message)
            .map_err(|failure| match failure {
                Failure::Stale(mut stale) => stale.remove(0).1,
                Failure::Error(error) => error,
            })
    }

    /// Reads JSON Lines of `{path, expect, record}` from `input`, checks
    /// every line and compares every expect under the lock, and writes them
    /// in order only if none refuses, with one commit.
    pub fn put_batch(&self, input: &mut dyn BufRead) -> Result<PutReport, PutError> {
        let mut prepared: Vec<Prepared> = Vec::new();
        let mut numbers: Vec<usize> = Vec::new();
        let mut refusals: Vec<Refusal> = Vec::new();
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
                refusals.push(Refusal {
                    line: number,
                    code: Code::JsonTooLarge,
                    message: format!("the line is over {MAX_RECORD_BYTES} bytes"),
                });
                break;
            }
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match self.prepare_line(text) {
                Ok(item) => {
                    if let Some(first) = prepared
                        .iter()
                        .position(|other| other.target.path == item.target.path)
                    {
                        refusals.push(Refusal {
                            line: number,
                            code: Code::PutInvalid,
                            message: format!(
                                "it writes {} as line {} does",
                                item.target.path, numbers[first]
                            ),
                        });
                    }
                    prepared.push(item);
                    numbers.push(number);
                }
                Err(error) => refusals.push(Refusal {
                    line: number,
                    code: error.code(),
                    message: error.to_string(),
                }),
            }
        }
        if !refusals.is_empty() {
            return Err(PutError::Batch {
                code: Code::PutInvalid,
                refusals,
            });
        }
        if prepared.is_empty() {
            return Ok(PutReport {
                writes: Vec::new(),
                commit: None,
            });
        }
        let message = format!("board put --batch ({} records)", prepared.len());
        self.write_all(&prepared, &message)
            .map_err(|failure| match failure {
                Failure::Stale(stale) => PutError::Batch {
                    code: Code::PutStale,
                    refusals: stale
                        .into_iter()
                        .map(|(at, error)| Refusal {
                            line: numbers[at],
                            code: error.code(),
                            message: error.to_string(),
                        })
                        .collect(),
                },
                Failure::Error(error) => error,
            })
    }

    /// Reads one batch line and prepares its record.
    fn prepare_line(&self, line: &[u8]) -> Result<Prepared, PutError> {
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
        self.prepare(&path, expect, &bytes)
    }

    /// Checks one record and finds where it lands, before any lock.
    fn prepare(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<Prepared, PutError> {
        let (value, is_pointer) = self.check_record(bytes).map_err(|code| PutError::Record {
            path: path.clone(),
            code,
        })?;
        let target = self.repository().target(path, is_pointer)?;
        Ok(Prepared {
            path: path.clone(),
            expect,
            canonical: canonical_json(&value),
            target,
        })
    }

    /// Takes the lock, compares every expect, and writes every prepared
    /// record in order, with one commit when `commit_on_put` is set. Writes
    /// nothing unless every expect holds.
    pub(crate) fn write_all(
        &self,
        prepared: &[Prepared],
        message: &str,
    ) -> Result<PutReport, Failure> {
        let repository = self.repository();
        let lock = repository
            .lock(LOCK_WAIT)
            .map_err(|error| Failure::Error(error.into()))?;
        let mut writes = Vec::with_capacity(prepared.len());
        let mut stale = Vec::new();
        for (at, item) in prepared.iter().enumerate() {
            let previous = repository
                .current_blob(&item.target.path)
                .map_err(|error| Failure::Error(error.into()))?;
            let matches = match item.expect {
                Expect::Any => true,
                Expect::Absent => previous.is_none(),
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
        Ok(PutReport { writes, commit })
    }

    /// Checks `bytes` as `check` checks a record, with its codes, and
    /// returns the parsed record and whether it is a pointer. Input that is
    /// not one JSON object with a `$type` is `put::invalid`.
    pub(crate) fn check_record(&self, bytes: &[u8]) -> Result<(Value, bool), Code> {
        let value = parse_json(bytes).map_err(|error| error.code())?;
        if value.get("$type").is_none() {
            return Err(Code::PutInvalid);
        }
        let classified = Role::Json.classify(Some(bytes));
        if let Some(code) = classified.code {
            return Err(code);
        }
        let record_type = match classified.content {
            Content::Pointer { .. } => return Ok((value, true)),
            Content::Record { record_type, .. } => record_type,
            Content::Json { .. }
            | Content::Schema { .. }
            | Content::File
            | Content::Unparsed(_) => {
                return Err(Code::PutInvalid);
            }
        };
        if record_type.as_str() == CONFIG_TYPE {
            return Config::from_value(&value)
                .map(|_| (value, false))
                .ok_or(Code::ConfigInvalid);
        }
        let schema_path = record_type.schema_path();
        let schema_bytes = self
            .repository()
            .read_working_file(&schema_path)
            .map_err(|_| Code::RepoUnreadable)?
            .ok_or(Code::RecordUnknownType)?;
        let schema = match Role::of(&schema_path).map(|role| role.classify(Some(&schema_bytes))) {
            Some(board_model::Classified {
                content: Content::Schema { value },
                code: None,
            }) => Schema::compile(value).map_err(|_| Code::SchemaInvalid)?,
            _ => return Err(Code::SchemaInvalid),
        };
        schema.validate(&value).map_err(|error| error.code())?;
        Ok((value, false))
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
    #[error("the record for {path} is refused: {code}")]
    Record { path: LogicalPath, code: Code },
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
            Self::Record { code, .. } | Self::Batch { code, .. } | Self::Line { code, .. } => *code,
            Self::TooManyLines => Code::PutInvalid,
            Self::Input { .. } => Code::RepoUnreadable,
            Self::Write(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests;

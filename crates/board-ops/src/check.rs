//! `board check`: every entry of the working tree, parsed and validated, with
//! its references and pointers followed (docs/architecture.md, Records;
//! Pointers: links and mounts).

use std::collections::HashMap;

use board_model::{
    CONFIG_TYPE, Classified, Code, Content, Kind, Location, LogicalPath, Object, POINTER_TYPE,
    RecordType, Source, classify, needs_bytes,
};
use board_tree::{Change, Entry, GitError, Repository, ResolveError, Status};
use board_validate::{Reference, Schema};
use serde_json::Value;

use crate::board::{Board, Config};

/// One entry as `check` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    pub path: String,
    pub logical: Option<LogicalPath>,
    pub kind: Kind,
    pub status: Status,
    pub change: Change,
}

/// Every entry `check` looked at, sorted by path.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckReport {
    pub entries: Vec<Checked>,
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
    /// Checks every entry of the working tree, or only those at or beneath
    /// `scope`. An entry's status is the first problem found: its path, then
    /// parsing, then its schema, validation and references, or for a pointer
    /// its target.
    pub fn check(&self, scope: Option<&LogicalPath>) -> Result<CheckReport, CheckError> {
        let source = Source::WorkingTree;
        if let Some(scope) = scope {
            self.repository()
                .resolve(&source, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let entries = self
            .repository()
            .walk(&source)
            .map_err(|source| CheckError::Git { source })?;
        let classified: Vec<Classified> =
            entries.iter().map(|entry| self.classify(entry)).collect();
        let schemas = compile_schemas(&entries, &classified);
        let checked = entries
            .into_iter()
            .zip(classified)
            .filter(|(entry, _)| within(entry.logical.as_ref(), scope))
            .map(|(entry, classified)| {
                let status = match entry.status {
                    Status::Ok => self.status(&entry, &classified, &schemas),
                    degraded @ Status::Degraded(_) => degraded,
                };
                Checked {
                    path: entry.path,
                    logical: entry.logical,
                    kind: classified.kind(),
                    status,
                    change: entry.change,
                }
            })
            .collect();
        Ok(CheckReport { entries: checked })
    }

    /// The entry's kind and what parsing found, reading it only when it is
    /// a `.json` file.
    fn classify(&self, entry: &Entry) -> Classified {
        if !needs_bytes(&entry.path) {
            return classify(&entry.path, None);
        }
        let read = entry.blob.ok_or(Code::RepoUnreadable).and_then(|blob| {
            self.repository()
                .read(&Source::WorkingTree, &entry.path, blob)
                .map_err(|error| error.code())
        });
        match read {
            Ok(bytes) => classify(&entry.path, bytes.as_deref()),
            Err(code) => Classified {
                content: Content::Unparsed(Kind::Json),
                code: Some(code),
            },
        }
    }

    /// The status of an entry whose path is fine.
    fn status(
        &self,
        entry: &Entry,
        classified: &Classified,
        schemas: &HashMap<String, Result<Schema, Code>>,
    ) -> Status {
        let code = classified.code.or_else(|| match &classified.content {
            Content::Schema { .. } => schemas.get(&entry.path)?.as_ref().err().copied(),
            Content::Pointer { .. } => {
                let logical = entry.logical.as_ref()?;
                self.repository()
                    .resolve(&Source::WorkingTree, logical)
                    .err()
                    .map(|error| error.code())
            }
            Content::Record { record_type, value } => {
                self.record(record_type, value, schemas).err()
            }
            Content::Json { .. } | Content::File | Content::Unparsed(_) => None,
        });
        code.map_or(Status::Ok, Status::Degraded)
    }

    /// Checks a record against its schema and its references.
    fn record(
        &self,
        record_type: &RecordType,
        value: &Value,
        schemas: &HashMap<String, Result<Schema, Code>>,
    ) -> Result<(), Code> {
        if record_type.as_str() == CONFIG_TYPE {
            return Config::from_value(value)
                .map(|_| ())
                .ok_or(Code::ConfigInvalid);
        }
        if record_type.is_board() {
            return Ok(());
        }
        let schema = match schemas.get(&record_type.schema_path()) {
            None => return Err(Code::RecordUnknownType),
            Some(Err(_)) => return Err(Code::SchemaInvalid),
            Some(Ok(schema)) => schema,
        };
        schema.validate(value).map_err(|error| error.code())?;
        schema
            .references(value)
            .iter()
            .try_for_each(|reference| self.reference(reference))
    }

    /// Checks that a reference resolves to something it accepts.
    fn reference(&self, reference: &Reference) -> Result<(), Code> {
        let location = self
            .repository()
            .resolve(&Source::WorkingTree, &reference.path)
            .map_err(|_| Code::RefDangling)?;
        let Some(accepts) = &reference.accepts else {
            return Ok(());
        };
        match record_type(&location) {
            Some(found) if accepts.contains(&found) => Ok(()),
            _ => Err(Code::RefWrongType),
        }
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

/// Every schema in the board, compiled, by physical path.
fn compile_schemas(
    entries: &[Entry],
    classified: &[Classified],
) -> HashMap<String, Result<Schema, Code>> {
    entries
        .iter()
        .zip(classified)
        .filter(|(_, classified)| classified.kind() == Kind::Schema)
        .map(|(entry, classified)| {
            let compiled = match classified {
                Classified {
                    content: Content::Schema { value },
                    code: None,
                } => Schema::compile(value.clone()).map_err(|error| error.code()),
                _ => Err(Code::SchemaInvalid),
            };
            (entry.path.clone(), compiled)
        })
        .collect()
}

/// The `$type` of the record at `location`, read in its own board.
fn record_type(location: &Location) -> Option<RecordType> {
    let Object::Blob(blob) = location.object else {
        return None;
    };
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
    Git { source: GitError },
}

impl CheckError {
    pub fn code(&self) -> Code {
        match self {
            Self::Scope { source } => source.code(),
            Self::Git { source } => source.code(),
        }
    }
}

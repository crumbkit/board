//! `board put`: checking a record and writing it in canonical form
//! (docs/architecture.md, Writes).

use board_model::{
    CONFIG_TYPE, Code, Content, Location, LogicalPath, Object, ObjectId, Role, canonical_json,
    parse_json,
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
        let refused = |code| PutError::Record {
            path: path.clone(),
            code,
        };
        let (value, is_pointer) = self.check_record(bytes).map_err(refused)?;
        let canonical = canonical_json(&value);
        let repository = self.repository();
        let target = repository.target(path, is_pointer)?;
        let lock = repository.lock(LOCK_WAIT)?;
        let previous = repository.current_blob(&target.path)?;
        let matches = match expect {
            Expect::Any => true,
            Expect::Absent => previous.is_none(),
            Expect::Blob(blob) => previous == Some(blob),
        };
        if !matches {
            return Err(PutError::Stale {
                path: path.clone(),
                found: previous,
            });
        }
        repository.write_file(&lock, &target.path, &canonical)?;
        drop(lock);
        let blob = repository.blob_id(&canonical);
        let location = Location {
            path: target.path,
            object: Object::Blob(blob),
            ..target.location
        };
        Ok(PutReport {
            writes: vec![Written {
                display: location.display_path().unwrap_or_else(|| path.clone()),
                location,
                previous,
                blob,
            }],
            commit: None,
        })
    }

    /// Checks `bytes` as `check` checks a record, with its codes, and
    /// returns the parsed record and whether it is a pointer. Input that is
    /// not one JSON object with a `$type` is `put::invalid`.
    fn check_record(&self, bytes: &[u8]) -> Result<(Value, bool), Code> {
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

/// A put that wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum PutError {
    #[error("the record for {path} is refused: {code}")]
    Record { path: LogicalPath, code: Code },
    #[error("{path} is not what --expect says: it holds {}", found.map_or_else(|| "nothing".to_owned(), |blob| blob.to_string()))]
    Stale {
        path: LogicalPath,
        found: Option<ObjectId>,
    },
    #[error(transparent)]
    Write(#[from] WriteError),
}

impl PutError {
    pub fn code(&self) -> Code {
        match self {
            Self::Record { code, .. } => *code,
            Self::Stale { .. } => Code::PutStale,
            Self::Write(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests;

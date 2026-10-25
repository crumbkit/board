//! Writing a type's schema: `board type add`, which adds a type to a board
//! from its schema (docs/architecture.md, Writes), and the schema writes
//! `plugin enable` makes.

use board_model::{
    Code, Fields, JsonError, LogicalPath, PathError, RecordType, Refusal, canonical_json,
    parse_json,
};
use board_tree::{LOCK_WAIT, Lock, Reader, Resolver, Working, WriteError};
use board_validate::{Schema, SchemaError};
use serde_json::Value;

use crate::Board;
use crate::put::{Expect, Failure, Prepared, PutError, PutReport};
use crate::verdict::Admitted;

impl Board {
    /// Compiles `bytes` as the schema of `record_type`, then writes it to
    /// `.board/types/` as one put that `commit_on_put` commits. An equal
    /// schema already there is left as it is, and committed if it was not,
    /// unless a symlink holds it; a different one is refused, since a
    /// changed type is a new version.
    pub fn type_add(&self, record_type: &RecordType, bytes: &[u8]) -> Result<PutReport, TypeError> {
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let reader = &mut self.repository().reader();
        let (Some(prepared), _) =
            Self::prepare_type(&lock, (&mut view, reader), record_type, bytes)?
        else {
            return Ok(PutReport::new(Vec::new(), None));
        };
        let message = format!("Add the type {record_type}");
        let mut report = self
            .write_all(lock, &[prepared], &[], &message)
            .map_err(Failure::one)?;
        report
            .writes
            .retain(|written| written.previous != Some(written.blob));
        Ok(report)
    }

    /// Compiles `bytes` as the schema of `record_type`, and finds where it
    /// lands in `view`, expecting no file when the board lacks it. Returns
    /// the write, if any, and the compiled schema. An equal schema is
    /// written again as the board holds it, in its own layout, which commits
    /// it if HEAD lacks it and is otherwise no write. An equal schema held
    /// through a symlink is not written, since no put writes over a
    /// symlink. A different schema is refused.
    pub(crate) fn prepare_type(
        lock: &Lock,
        (view, reader): (&mut Resolver<'_, '_>, &mut Reader<'_>),
        record_type: &RecordType,
        bytes: &[u8],
    ) -> Result<(Option<Prepared>, Schema), TypeError> {
        let record_type = record_type.clone();
        if record_type.is_board() {
            return Err(TypeError::Reserved { record_type });
        }
        let schema = match parse_json(bytes) {
            Ok(schema) => schema,
            Err(source) => {
                return Err(TypeError::Json {
                    record_type,
                    source,
                });
            }
        };
        let compiled = match Schema::compile(schema.clone()) {
            Ok(compiled) => compiled,
            Err(source) => {
                return Err(TypeError::Schema {
                    record_type,
                    source,
                });
            }
        };
        let Ok(path) = schema_at(&record_type) else {
            return Err(TypeError::TooLong { record_type });
        };
        let (expect, held) = match Self::held(reader, &record_type, &schema)? {
            Held::Absent => (Expect::Absent, None),
            Held::Same(bytes) => (Expect::Any, Some(bytes)),
            Held::Linked => return Ok((None, compiled)),
            Held::Other => return Err(TypeError::Differs { record_type }),
            Held::Unparsed(source) => {
                return Err(TypeError::Held {
                    record_type,
                    source,
                });
            }
        };
        let mut prepared = Self::schema(lock, view, path, expect, &schema)?;
        prepared.canonical = held.unwrap_or(prepared.canonical);
        Ok((Some(prepared), compiled))
    }

    /// How the working tree holds `schema` for `record_type`.
    pub(crate) fn held(
        reader: &mut Reader<'_>,
        record_type: &RecordType,
        schema: &Value,
    ) -> Result<Held, WriteError> {
        let (working, linked) = reader.read_through(&record_type.schema_path())?;
        Ok(match working {
            Working::Missing => Held::Absent,
            Working::Read(contents) => match parse_json(&contents.bytes) {
                Ok(held) if held == *schema && linked => Held::Linked,
                Ok(held) if held == *schema => Held::Same(contents.bytes),
                Ok(_) => Held::Other,
                Err(error) => Held::Unparsed(error),
            },
            Working::TooLarge => Held::Other,
        })
    }

    /// A schema, to be written at `path` over what `expect` says is there.
    pub(crate) fn schema(
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        path: LogicalPath,
        expect: Expect,
        schema: &Value,
    ) -> Result<Prepared, WriteError> {
        let target = view.target(&path, false)?;
        Ok(Prepared {
            path,
            expect,
            canonical: canonical_json(schema).into_bytes(),
            target,
            admitted: Admitted {
                value: schema.clone(),
                pointer: false,
                references: Vec::new(),
            },
        })
    }
}

/// The logical path of `record_type`'s schema, which a put names.
pub(crate) fn schema_at(record_type: &RecordType) -> Result<LogicalPath, PathError> {
    let physical = record_type.schema_path();
    let stem = physical.strip_suffix(".json").unwrap_or(&physical);
    format!("/{stem}").parse()
}

/// How the board holds a type's schema: not at all, as an equal schema
/// with the file's bytes, as an equal schema through a symlink, as another
/// one, or as a file that is not JSON.
pub(crate) enum Held {
    Absent,
    Same(Vec<u8>),
    Linked,
    Other,
    Unparsed(JsonError),
}

/// A type add that wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("{record_type} is in the `board` namespace, which holds only Board's own types")]
    Reserved { record_type: RecordType },
    #[error("the schema for {record_type} is not JSON: {source}")]
    Json {
        record_type: RecordType,
        source: JsonError,
    },
    #[error("the {record_type} {source}")]
    Schema {
        record_type: RecordType,
        source: SchemaError,
    },
    #[error(
        "the schema path of {record_type}, {}, has a segment over 255 bytes",
        record_type.schema_path()
    )]
    TooLong { record_type: RecordType },
    #[error(
        "the board already holds another schema for {record_type} at {}",
        record_type.schema_path()
    )]
    Differs { record_type: RecordType },
    #[error(
        "the board's schema for {record_type} at {} is not JSON: {source}",
        record_type.schema_path()
    )]
    Held {
        record_type: RecordType,
        source: JsonError,
    },
    #[error(transparent)]
    Put(#[from] PutError),
}

impl TypeError {
    pub fn code(&self) -> Code {
        match self {
            Self::Reserved { .. } => Code::SchemaInvalid,
            Self::Json { source, .. } | Self::Held { source, .. } => source.code(),
            Self::Schema { source, .. } => source.code(),
            Self::TooLong { .. } => Code::PathMalformed,
            Self::Differs { .. } => Code::SchemaDiffers,
            Self::Put(error) => error.code(),
        }
    }

    /// The type, and the file that holds the board's copy when the
    /// refusal is about it.
    pub fn fields(&self) -> Fields {
        let (record_type, schema) = match self {
            Self::Put(error) => return error.fields(),
            Self::Differs { record_type } | Self::Held { record_type, .. } => {
                (record_type, Some(record_type.schema_path()))
            }
            Self::Reserved { record_type }
            | Self::Json { record_type, .. }
            | Self::Schema { record_type, .. }
            | Self::TooLong { record_type } => (record_type, None),
        };
        Fields {
            record_type: Some(record_type.clone()),
            schema,
            ..Fields::default()
        }
    }

    pub fn refusal(&self) -> Refusal {
        let refusal = Refusal::new(self.code(), self, &self.fields());
        match self {
            Self::Reserved { .. } => {
                refusal.with_hint("name the type in a namespace of your own, such as notes/note/v1")
            }
            Self::TooLong { .. } => {
                refusal.with_hint("shorten the type's name to parts of at most 255 bytes each")
            }
            _ => refusal,
        }
    }
}

impl From<WriteError> for TypeError {
    fn from(error: WriteError) -> Self {
        Self::Put(error.into())
    }
}

#[cfg(test)]
mod tests;

//! JSON Schema validation of records against schemas from the tree
//! (docs/architecture.md, Records; Typed references). Schemas from a mounted
//! board are untrusted: patterns run on a linear-time engine and no `$ref`
//! is ever fetched.

mod references;

use board_model::{Code, LogicalPath, RecordType};
use jsonschema::{
    Draft, PatternOptions, Retrieve, Uri, ValidationError, Validator, error::ValidationErrorKind,
};
use serde_json::Value;

pub use crate::references::Reference;

/// The only draft Board reads.
const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// The format that marks a string as a reference to a logical path.
const REFERENCE_FORMAT: &str = "board-ref";

/// A compiled schema.
#[derive(Debug)]
pub struct Schema {
    document: Value,
    validator: Validator,
}

impl Schema {
    /// Compiles a schema document. It must be JSON Schema 2020-12, and its
    /// `x-board-type` values must be record types.
    pub fn compile(document: Value) -> Result<Self, SchemaError> {
        if let Some(draft) = document.get("$schema")
            && draft.as_str() != Some(DRAFT_2020_12)
        {
            return Err(SchemaError::Draft {
                draft: draft.to_string(),
            });
        }
        references::check_types(&document)?;
        references::check_expansion(&document)?;
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .with_pattern_options(PatternOptions::regex())
            .with_retriever(Refuse)
            .with_format(REFERENCE_FORMAT, |text: &str| {
                text.parse::<LogicalPath>().is_ok()
            })
            .should_validate_formats(true)
            .build(&document)
            .map_err(|source| SchemaError::Compile {
                source: Box::new(source),
            })?;
        Ok(Self {
            document,
            validator,
        })
    }

    /// The schema as written.
    pub fn document(&self) -> &Value {
        &self.document
    }

    /// Checks `record` against the schema. Of every failure, the one at the
    /// first instance location in sorted order is reported, so the report
    /// does not depend on the validator's order.
    pub fn validate(&self, record: &Value) -> Result<(), RecordError> {
        let first = self
            .validator
            .iter_errors(record)
            .min_by_key(|error| error.instance_path().to_string());
        let Some(error) = first else {
            return Ok(());
        };
        let pointer = error.instance_path().to_string();
        match error.kind() {
            ValidationErrorKind::Format { format } if format == REFERENCE_FORMAT => {
                Err(RecordError::MalformedReference { pointer })
            }
            _ => Err(RecordError::Invalid {
                pointer,
                source: Box::new(error.to_owned()),
            }),
        }
    }

    /// The references `record` makes, in document order. Call it on a record
    /// that validates.
    pub fn references(&self, record: &Value) -> Vec<Reference> {
        references::collect(&self.document, record)
    }
}

/// Refuses every `$ref` that is not inside the schema itself.
struct Refuse;

impl Retrieve for Refuse {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("{uri} is outside the schema, and Board fetches nothing").into())
    }
}

/// A schema that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema declares draft {draft}, but only JSON Schema 2020-12 is read")]
    Draft { draft: String },
    #[error("schema x-board-type {value} is not a record type or a list of them")]
    BoardType { value: String },
    #[error(
        "schema expands to more than {} subschemas at one record location through allOf and $ref",
        references::MAX_EXPANSION
    )]
    TooComplex,
    #[error("schema does not compile: {source}")]
    Compile {
        #[source]
        source: Box<ValidationError<'static>>,
    },
}

impl SchemaError {
    pub fn code(&self) -> Code {
        match self {
            Self::Draft { .. }
            | Self::BoardType { .. }
            | Self::TooComplex
            | Self::Compile { .. } => Code::SchemaInvalid,
        }
    }
}

/// A record its schema refuses.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("record fails its schema at {pointer:?}: {source}")]
    Invalid {
        pointer: String,
        #[source]
        source: Box<ValidationError<'static>>,
    },
    #[error("record holds a reference at {pointer:?} that is not a logical path")]
    MalformedReference { pointer: String },
}

impl RecordError {
    pub fn code(&self) -> Code {
        match self {
            Self::Invalid { .. } => Code::RecordInvalid,
            Self::MalformedReference { .. } => Code::RefMalformed,
        }
    }
}

/// The record types a reference accepts, from `x-board-type`; `None` when
/// the schema names none.
pub type Accepts = Option<Vec<RecordType>>;

#[cfg(test)]
mod tests;

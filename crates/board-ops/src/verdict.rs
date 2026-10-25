//! What a blob's bytes say about it, and what a record's schema says about
//! the record, decided once for the index and for `put`
//! (docs/architecture.md, Records).

use board_model::{
    ATTACHMENT_TYPE, Attachment, BINDING_TYPE, Binding, CONFIG_TYPE, Classified, Code, Content,
    Fields, MANIFEST_TYPE, RecordType, Role, UiError, effect_record,
};
use board_plugin::{Manifest, ManifestError};
use board_tree::Refused;
use board_validate::{RecordError, Reference, Schema, SchemaError};
use serde_json::Value;

use crate::board::Config;

/// Why a blob or a record cannot be used, whatever else is in the tree.
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    #[error("{0}")]
    Code(Code),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Record(#[from] RecordError),
    /// A record of one of Board's effect types that does not read as its
    /// Rust type, with the reason it gives.
    #[error("it is not a valid {record_type}: {why}")]
    Core {
        record_type: RecordType,
        why: String,
    },
    #[error(transparent)]
    Ui(#[from] UiError),
    /// A record of a user's type with no usable schema at its schema path.
    #[error("{}", match found {
        SchemaFound::Missing => format!("no schema for {record_type} at {}", record_type.schema_path()),
        SchemaFound::Unreadable => format!("its schema at {} could not be read", record_type.schema_path()),
        SchemaFound::Unusable => format!("its schema at {} does not compile", record_type.schema_path()),
        SchemaFound::Symlink(refused) => format!("its schema at {} is {refused}", record_type.schema_path()),
    })]
    NoSchema {
        record_type: RecordType,
        found: SchemaFound,
    },
}

impl Fault {
    pub fn code(&self) -> Code {
        match self {
            Self::Code(code) => *code,
            Self::Manifest(error) => error.code(),
            Self::Schema(error) => error.code(),
            Self::Record(error) => error.code(),
            Self::Core { .. } => Code::RecordInvalid,
            Self::Ui(error) => error.code(),
            Self::NoSchema { found, .. } => match found {
                SchemaFound::Missing => Code::RecordUnknownType,
                SchemaFound::Unreadable => Code::RepoUnreadable,
                SchemaFound::Unusable => Code::SchemaInvalid,
                SchemaFound::Symlink(refused) => refused.code,
            },
        }
    }

    /// The pointer a schema refused, or the type and schema file of a
    /// record with no usable schema, and `/$type` when that schema does not
    /// compile.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Record(error) => error.fields(),
            Self::Ui(error) => Fields {
                pointer: error.pointer(),
                ..Fields::default()
            },
            Self::NoSchema { record_type, found } => Fields {
                pointer: (*found == SchemaFound::Unusable).then(|| "/$type".to_owned()),
                schema: Some(record_type.schema_path()),
                target: match found {
                    SchemaFound::Symlink(refused) => Some(refused.target.clone()),
                    _ => None,
                },
                record_type: Some(record_type.clone()),
                ..Fields::default()
            },
            Self::Core { record_type, .. } => Fields {
                record_type: Some(record_type.clone()),
                ..Fields::default()
            },
            Self::Code(_) | Self::Manifest(_) | Self::Schema(_) => Fields::default(),
        }
    }

    /// Why a blob with `bytes`, read in `role`, fails, when Board can say
    /// more than the code:
    /// the parser's message, which this parses `bytes` again for, that a
    /// schema is in the reserved namespace, the only schema `judge` refuses
    /// by its code, or the fault's own.
    pub(crate) fn reason(&self, role: Role, bytes: Option<&[u8]>) -> Option<String> {
        match self {
            Self::Code(Code::SchemaInvalid) => Some(
                "is in a namespace Board reserves, board or ui, so a board cannot define a schema in it"
                    .to_owned(),
            ),
            Self::Code(_) => Some(format!("is invalid: {}", role.parse_error(bytes?)?.1)),
            fault => Some(format!("is invalid: {fault}")),
        }
    }
}

/// What is at a record type's schema path when it is not a usable schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaFound {
    Missing,
    /// A file that could not be read, or that changed while it was read.
    Unreadable,
    /// A file that is not a schema that compiles.
    Unusable,
    /// A symlink that leads to no file.
    Symlink(Refused),
}

/// Judges a classified blob by its bytes alone: its classification's code,
/// else a schema must compile, and Board's configuration and plugin
/// manifests must read as their Rust types. Returns a schema compiled.
pub(crate) fn judge(classified: &Classified) -> Result<Option<Schema>, Fault> {
    if let Some(code) = classified.code {
        return Err(Fault::Code(code));
    }
    match &classified.content {
        Content::Schema { value } => Ok(Some(Schema::compile(value.clone())?)),
        Content::Record { record_type, value } if record_type.as_str() == CONFIG_TYPE => {
            Config::from_value(value)
                .map(|_| None)
                .ok_or(Fault::Code(Code::ConfigInvalid))
        }
        Content::Record { record_type, value } if record_type.as_str() == MANIFEST_TYPE => {
            Manifest::from_value(value.clone())?;
            Ok(None)
        }
        Content::Record { record_type, value } if record_type.as_str() == ATTACHMENT_TYPE => {
            Attachment::from_value(value)
                .map(|_| None)
                .map_err(|_| Fault::Code(Code::RecordInvalid))
        }
        Content::Record { record_type, value } => {
            if let Some(Err(why)) = effect_record(record_type.as_str(), value) {
                return Err(Fault::Core {
                    record_type: record_type.clone(),
                    why,
                });
            }
            board_model::check_ui(record_type.as_str(), value).transpose()?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Validates `record` against `schema`, and returns the references it makes.
pub(crate) fn against(schema: &Schema, record: &Value) -> Result<Vec<Reference>, RecordError> {
    schema.validate(record)?;
    Ok(schema.references(record))
}

/// A record admitted: its value, and what `check` follows from it once
/// written, its target when it is a pointer or else its references.
#[derive(Debug)]
pub(crate) struct Admitted {
    pub(crate) value: Value,
    pub(crate) pointer: bool,
    pub(crate) references: Vec<Reference>,
}

impl Admitted {
    /// The binding this record is, when it is one.
    pub(crate) fn binding(&self) -> Option<Binding> {
        (self.value.get("$type").and_then(Value::as_str) == Some(BINDING_TYPE))
            .then(|| Binding::from_value(&self.value).ok())
            .flatten()
    }
}

/// Judges a classified record by its bytes, then a record of a user's type
/// against the schema `find` gives for its type. Board's own types that
/// pass `judge` need no schema.
pub(crate) fn record<'s>(
    classified: Classified,
    find: impl FnOnce(&RecordType) -> Result<&'s Schema, SchemaFound>,
) -> Result<Admitted, Fault> {
    judge(&classified)?;
    let admitted = |value, pointer| Admitted {
        value,
        pointer,
        references: Vec::new(),
    };
    let (record_type, value) = match classified.content {
        Content::Pointer { value, .. } => return Ok(admitted(value, true)),
        Content::Record { record_type, value } if record_type.is_known() => {
            return Ok(admitted(value, false));
        }
        Content::Record { record_type, value } => (record_type, value),
        Content::Json { .. } | Content::Schema { .. } | Content::File | Content::Unparsed(_) => {
            return Err(Fault::Code(Code::PutInvalid));
        }
    };
    let schema = find(&record_type).map_err(|found| Fault::NoSchema { record_type, found })?;
    let references = against(schema, &value)?;
    Ok(Admitted {
        references,
        ..admitted(value, false)
    })
}

#[cfg(test)]
mod tests;

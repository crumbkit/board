//! What a blob's bytes alone say about it, decided once for the index and
//! for `put` (docs/architecture.md, Records).

use board_model::{CONFIG_TYPE, Classified, Code, Content, MANIFEST_TYPE};
use board_plugin::{Manifest, ManifestError};
use board_validate::{Schema, SchemaError};

use crate::board::Config;

/// Why a blob cannot be used, whatever else is in the tree.
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    #[error("{0}")]
    Code(Code),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

impl Fault {
    pub fn code(&self) -> Code {
        match self {
            Self::Code(code) => *code,
            Self::Manifest(error) => error.code(),
            Self::Schema(error) => error.code(),
        }
    }
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
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;

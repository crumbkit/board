//! The refusals of the effects check, and the requests it could not
//! finish.

use board_model::{
    Code, Fields, LogicalPath, MAX_DECIDE_BYTES, MAX_RECORD_BYTES, PathError, PluginName, Refusal,
};
use board_tree::{GitError, WriteError};

use super::PENDING;
use crate::board::OpenError;
use crate::put::PutError;

/// A request refused by the effects check, or one it could not finish.
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("plugin {plugin} may not {op} {path} until the owner answers pending effect {id}")]
    Asked {
        plugin: PluginName,
        op: String,
        path: LogicalPath,
        id: String,
    },
    #[error("plugin {plugin} may not {op} {path}: the owner refused pending effect {id}")]
    Refused {
        plugin: PluginName,
        op: String,
        path: LogicalPath,
        id: String,
    },
    #[error("plugin {plugin} may not {what}; only the owner may, outside a plugin")]
    FromPlugin {
        plugin: PluginName,
        path: Option<LogicalPath>,
        what: String,
    },
    #[error(
        "{path} lands at a file with no logical path, so no pending effect can name it: {source}"
    )]
    Unlanded {
        path: LogicalPath,
        source: PathError,
    },
    #[error("{what}, but the known boards file names no personal board")]
    NoPersonal { what: String },
    #[error("the record at {path} in the personal board is over {MAX_RECORD_BYTES} bytes")]
    TooLarge { path: String },
    #[error("{why}")]
    Unshown { code: Code, why: String },
    #[error("{source}")]
    Resolve { source: Box<WriteError> },
    #[error("no pending effect {id} is in the personal board")]
    NotFound { id: String },
    #[error("the record at /{PENDING}/{id} is not the pending effect {id} names: {why}")]
    Forged { id: String, why: String },
    #[error("pending effect {id} has no good approval signature: {why}")]
    Unsigned { id: String, why: String },
    #[error(
        "the owner's {op} of {path} changes an effect or the board's config and has no good signature: {why}"
    )]
    OwnerUnsigned {
        op: &'static str,
        path: String,
        why: String,
    },
    #[error(
        "{what} needs the owner's signature and {file} is missing; Board has verified a signature before, so it does not take an answer instead"
    )]
    FileMissing { what: String, file: String },
    #[error("{what} is not approved: {why}")]
    NotApproved { what: String, why: String },
    #[error("the records in {} cannot be read: {source}", directory)]
    Read { directory: String, source: GitError },
    #[error("the personal board cannot be opened: {source}")]
    Open { source: OpenError },
    #[error("signing's state reads the personal board, which cannot be named: {}", refusal.message)]
    Personal { refusal: Box<Refusal> },
    #[error("the personal board refused a write: {source}")]
    Write { source: Box<PutError> },
}

impl EffectError {
    pub fn code(&self) -> Code {
        match self {
            Self::Asked { .. } => Code::SettingsAsked,
            Self::Refused { .. } => Code::SettingsRefused,
            Self::FromPlugin { .. } => Code::SettingsFromPlugin,
            Self::NoPersonal { .. } => Code::SettingsNoPersonal,
            Self::Unlanded { source, .. } => source.code(),
            Self::TooLarge { .. } => Code::JsonTooLarge,
            Self::Unshown { code, .. } => *code,
            Self::Resolve { source } => source.code(),
            Self::NotFound { .. } | Self::Forged { .. } => Code::SettingsNotFound,
            Self::Unsigned { .. } | Self::OwnerUnsigned { .. } => Code::SettingsUnsigned,
            Self::FileMissing { .. } => Code::SigningFileMissing,
            Self::NotApproved { .. } => Code::SigningNotApproved,
            Self::Read { source, .. } => source.code(),
            Self::Open { source } => source.code(),
            Self::Personal { refusal } => match &refusal.code {
                board_model::RefusalCode::Board(code) => *code,
                board_model::RefusalCode::Plugin(_) => Code::ConfigInvalid,
            },
            Self::Write { source } => source.code(),
        }
    }

    pub fn fields(&self) -> Fields {
        match self {
            Self::Asked {
                plugin, path, id, ..
            }
            | Self::Refused {
                plugin, path, id, ..
            } => Fields {
                plugin: Some(plugin.clone()),
                effect: Some(id.clone()),
                ..Fields::at(path)
            },
            Self::FromPlugin { plugin, path, .. } => Fields {
                plugin: Some(plugin.clone()),
                path: path.clone(),
                ..Fields::default()
            },
            Self::Unlanded { path, .. } => Fields::at(path),
            Self::NotFound { id } | Self::Forged { id, .. } | Self::Unsigned { id, .. } => Fields {
                effect: Some(id.clone()),
                ..Fields::default()
            },
            Self::OwnerUnsigned { path, .. } => Fields {
                path: path.parse().ok(),
                ..Fields::default()
            },
            Self::Open { source } => source.fields(),
            Self::Write { source } => source.fields(),
            Self::Resolve { source } => source.fields(),
            Self::Read { .. }
            | Self::FileMissing { .. }
            | Self::NotApproved { .. }
            | Self::NoPersonal { .. }
            | Self::Personal { .. }
            | Self::TooLarge { .. }
            | Self::Unshown { .. } => Fields::default(),
        }
    }

    pub fn refusal(&self) -> Refusal {
        let refusal = Refusal::new(self.code(), self, &self.fields());
        let shown = [Code::JsonTooLarge, Code::RecordNotARecord].contains(&self.code());
        match self {
            Self::Personal { refusal } => (**refusal).clone(),
            Self::Unshown { .. } if shown => refusal.with_hint(format!("a decide shows the owner a JSON object, or UTF-8 text, of at most {MAX_DECIDE_BYTES} bytes; give the content the op will write")),
            _ => refusal,
        }
    }
}

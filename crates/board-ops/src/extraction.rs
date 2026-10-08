//! Extraction before `query` and `call` (docs/plugins.md, Extraction).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use board_index::{Index, IndexError, PluginDatabase, Storage};
use board_model::{Code, RecordType, Source, parse_json};
use board_plugin::{
    CallError, Context, Environment, Installed, Limits, Manifest, Record, extract, installed,
};
use board_tree::Repository;
use serde_json::{Map, Value};

use crate::board::Board;
use crate::check::CheckError;

/// Where plugins are installed, what they inherit and what a run may take.
#[derive(Clone, Debug, Default)]
pub struct Plugins<'a> {
    pub directory: Option<&'a Path>,
    pub environment: Environment,
    pub limits: Limits,
}

impl Plugins<'_> {
    /// Every installed plugin; none without a plugins directory.
    pub(crate) fn installed(&self) -> Result<Vec<Installed>, CallError> {
        match self.directory {
            Some(directory) => {
                installed(directory).map_err(|source| CallError::Discovery { source })
            }
            None => Ok(Vec::new()),
        }
    }
}

/// A plugin's database for this run.
pub(crate) fn database(
    storage: &Storage,
    manifest: &Manifest,
) -> Result<PluginDatabase, CallError> {
    PluginDatabase::open(
        storage,
        &manifest.name.to_string(),
        &manifest.version.to_string(),
    )
    .map_err(|source| CallError::Database {
        plugin: manifest.name.clone(),
        source,
    })
}

impl Board {
    /// Sends `manifest`'s plugin, in `directory`, every current record of its
    /// types it has not handled, and records what it handled. A database the
    /// index did not record makes it forget what it handled before. When the
    /// database's id cannot be read or made, it sends nothing this run.
    pub(crate) fn extract_into(
        &self,
        index: &Index,
        manifest: &Manifest,
        directory: &Path,
        database: &PluginDatabase,
        plugins: &Plugins<'_>,
    ) -> Result<(), CallFailed> {
        // A plugin that extracts nothing needs no id.
        let Some(id) = database.id().filter(|_| !manifest.extract.is_empty()) else {
            return Ok(());
        };
        let (name, version) = (manifest.name.as_str(), manifest.version.as_str());
        index.trust(name, version, id)?;
        let types: Vec<&str> = manifest.extract.iter().map(RecordType::as_str).collect();
        let mut mounted: HashMap<PathBuf, Option<Repository>> = HashMap::new();
        let records: Vec<Record> = index
            .unhandled(name, version, &types)?
            .into_iter()
            .filter_map(|unhandled| {
                let repository = match unhandled.source {
                    Source::WorkingTree => self.repository(),
                    Source::Commit(_) => mounted
                        .entry(unhandled.repository.clone())
                        .or_insert_with(|| Repository::open(&unhandled.repository).ok())
                        .as_ref()?,
                };
                // A file that changed since the refresh is sent next time.
                let contents = repository
                    .read(&unhandled.source, &unhandled.path, unhandled.blob)
                    .ok()??;
                Some(Record {
                    blob: unhandled.blob,
                    path: unhandled.logical,
                    record_type: unhandled.record_type,
                    record: parse_json(&contents.bytes).ok()?,
                })
            })
            .collect();
        let context = Context {
            root: self.repository().path(),
            database: database.path(),
            environment: &plugins.environment,
            limits: plugins.limits,
        };
        let extraction = extract(manifest, directory, records, &context);
        index.handled(name, version, id, &extraction.handled)?;
        match extraction.failure {
            Some(failure) => Err(failure.into()),
            None => Ok(()),
        }
    }
}

/// A call or extraction that failed: the plugin's or Board's refusal, or the
/// index or check it needed.
#[derive(Debug, thiserror::Error)]
pub enum CallFailed {
    #[error(transparent)]
    Call {
        #[from]
        source: CallError,
    },
    #[error(transparent)]
    Index {
        #[from]
        source: IndexError,
    },
    #[error(transparent)]
    Check {
        #[from]
        source: CheckError,
    },
}

impl CallFailed {
    /// The code as `board/error/v1` spells it: Board's or the plugin's own.
    pub fn code_text(&self) -> &str {
        match self {
            Self::Call { source } => source.code_text(),
            Self::Index { source } => source.code().as_str(),
            Self::Check { source } => source.code().as_str(),
        }
    }

    /// Board's code; `None` for a plugin's own refusal.
    pub fn code(&self) -> Option<Code> {
        match self {
            Self::Call { source } => source.code(),
            Self::Index { source } => Some(source.code()),
            Self::Check { source } => Some(source.code()),
        }
    }

    pub fn fields(&self) -> Map<String, Value> {
        match self {
            Self::Call { source } => source.fields(),
            Self::Index { .. } | Self::Check { .. } => Map::new(),
        }
    }

    pub fn stderr(&self) -> Option<&[u8]> {
        match self {
            Self::Call { source } => source.stderr(),
            Self::Index { .. } | Self::Check { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;

//! Extraction before `query` (docs/plugins/extraction.md).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use board_index::{DatabaseError, IndexError, PluginDatabase, Storage, View};
use board_model::{Fields, RecordType, Refusal, parse_json};
use board_plugin::{
    CallError, Context, Environment, Installed, Limits, Manifest, PluginName, Record, extract,
    installed,
};

use crate::board::Board;
use crate::plugins::SchemaProblem;

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
    .map_err(|error| {
        let plugin = manifest.name.clone();
        match error {
            DatabaseError::File { path, source } => CallError::Database {
                plugin,
                path,
                source,
            },
            DatabaseError::TemporaryDirectory { parent, source } => {
                CallError::NoTemporaryDirectory {
                    plugin,
                    parent,
                    source,
                }
            }
        }
    })
}

impl Board {
    /// Sends `manifest`'s plugin, in `directory`, every current record of its
    /// types it has not handled, and records what it handled. A database the
    /// index did not record makes it forget what it handled before. When the
    /// database's id cannot be read or made, it sends nothing this run.
    pub(crate) fn extract_into(
        &self,
        view: &View,
        manifest: &Manifest,
        directory: &Path,
        database: &PluginDatabase,
        plugins: &Plugins<'_>,
    ) -> Result<(), CallFailed> {
        // A plugin that extracts nothing needs no id.
        let Some(id) = database.id().filter(|_| !manifest.extract.is_empty()) else {
            return Ok(());
        };
        let _timed = self.timed("plugin.extract");
        let (name, version) = (manifest.name.as_str(), manifest.version.as_str());
        view.trust(name, version, id)?;
        let types: Vec<&str> = manifest.extract.iter().map(RecordType::as_str).collect();
        let records: Vec<Record> = view
            .unhandled(name, version, &types)?
            .into_iter()
            .filter_map(|unhandled| {
                let board = board_model::Board {
                    repository: unhandled.repository,
                    source: unhandled.source,
                };
                let repository = self.repositories().repository(&board).ok()?;
                // A file that changed since the refresh is sent next time.
                let contents = repository
                    .read(&board.source, &unhandled.path, unhandled.blob)
                    .ok()??;
                Some(Record {
                    blob: unhandled.blob,
                    path: unhandled.logical,
                    record_type: unhandled.record_type,
                    record: parse_json(&contents.bytes).ok()?,
                })
            })
            .collect();
        let spawned = RefCell::default();
        let context = Context {
            root: self.repository().path(),
            database: database.path(),
            environment: &plugins.environment,
            limits: plugins.limits,
            spawned: &spawned,
        };
        let extraction = extract(manifest, directory, records, &context);
        self.trace.borrow_mut().started(spawned.take());
        view.handled(name, version, id, &extraction.handled)?;
        match extraction.failure {
            Some(failure) => Err(failure.into()),
            None => Ok(()),
        }
    }
}

/// A call or extraction that failed: the plugin's or Board's refusal, or the
/// index it needed.
#[derive(Debug, thiserror::Error)]
pub enum CallFailed {
    #[error(
        "the {plugin} plugin ships {} at {}, and the board does not hold the same schema at {}",
        problem.record_type,
        shipped.display(),
        problem.path
    )]
    Schema {
        plugin: PluginName,
        shipped: PathBuf,
        problem: SchemaProblem,
    },
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
}

impl CallFailed {
    /// This failure as `board/error/v1` reports it: Board's code or the
    /// plugin's own, with the end of the plugin's stderr when it ran.
    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Schema {
                plugin, problem, ..
            } => Refusal::new(
                problem.code,
                self,
                &Fields {
                    plugin: Some(plugin.to_string()),
                    record_type: Some(problem.record_type.clone()),
                    schema: Some(problem.path.clone()),
                    ..Fields::default()
                },
            ),
            Self::Call { source } => source.refusal(),
            Self::Index { source } => Refusal::new(source.code(), source, &Fields::default()),
        }
    }
}

#[cfg(test)]
mod tests;

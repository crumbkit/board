//! `plugin enable`: adding a plugin to a board with the schemas it ships
//! (docs/plugins.md, Installing and enabling).

use std::path::Path;

use board_model::{Code, RecordType, canonical_json};
use board_plugin::{DiscoveryError, ManifestError, PluginName, installed};
use board_tree::{LOCK_WAIT, WriteError};

use crate::Board;
use crate::board::config_file;
use crate::put::{Expect, Failure, PutError, PutReport, Schemas};
use crate::types::{Held, schema_at};

/// What `plugin enable` wrote: each schema the board lacked or held
/// otherwise, then `.board/board.json` when it did not name the plugin.
#[derive(Debug)]
pub struct EnableReport {
    pub name: PluginName,
    pub put: PutReport,
    /// Each schema the board held otherwise, by its path from the root.
    pub replaced: Vec<String>,
}

impl Board {
    /// Enables the plugin `name` installed in `plugins`: writes each schema
    /// it ships to `.board/types/`, then adds it to `plugins` in
    /// `.board/board.json`, as one put. A schema the board already holds is
    /// left as it is; a different one is replaced, since the plugin's copy is
    /// the source. The board is read under the lock the write holds.
    pub fn plugin_enable(
        &self,
        plugins: Option<&Path>,
        name: &PluginName,
    ) -> Result<EnableReport, EnableError> {
        let found = match plugins {
            Some(plugins) => installed(plugins)?,
            None => Vec::new(),
        }
        .into_iter()
        .find(|installed| installed.name == name.as_str())
        .ok_or_else(|| EnableError::NotInstalled { name: name.clone() })?;
        if let Err(source) = found.manifest {
            return Err(EnableError::Manifest {
                name: name.clone(),
                source,
            });
        }
        let lock = self.repository().lock(LOCK_WAIT)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let (mut prepared, mut replaced) = (Vec::new(), Vec::new());
        for (record_type, shipped) in &found.schemas {
            let expect = match self.held(record_type, shipped)? {
                Held::Same(_) => continue,
                Held::Absent => Expect::Absent,
                Held::Other | Held::Unparsed(_) => {
                    replaced.push(record_type.schema_path());
                    Expect::Any
                }
            };
            let path = schema_at(record_type).map_err(|_| EnableError::Schema {
                name: name.clone(),
                record_type: record_type.clone(),
            })?;
            prepared.push(Self::schema(&lock, &mut view, path, expect, shipped)?);
        }
        // The configuration is read again here, so a plugin another process
        // enabled since the board was opened is kept.
        let changed = || EnableError::Changed { name: name.clone() };
        let (blob, mut config, current) = config_file(self.repositories()).ok_or_else(changed)?;
        if !current.plugins.contains(name) {
            let fields = config.as_object_mut().ok_or_else(changed)?;
            let mut enabled = current.plugins;
            enabled.push(name.clone());
            fields.insert("plugins".to_owned(), serde_json::json!(enabled));
            let path = "/.board/board".parse().map_err(|_| changed())?;
            prepared.push(self.prepare(
                &lock,
                &mut view,
                &mut Schemas::default(),
                &path,
                Expect::Blob(blob),
                &canonical_json(&config).into_bytes(),
            )?);
        }
        if prepared.is_empty() {
            return Ok(EnableReport {
                name: name.clone(),
                put: PutReport {
                    writes: Vec::new(),
                    commit: None,
                },
                replaced,
            });
        }
        let put = self
            .write_all(lock, &prepared, &format!("Enable the {name} plugin"))
            .map_err(|failure| match failure {
                Failure::Error(error) => EnableError::Put(error),
                Failure::Stale(_) => EnableError::Changed { name: name.clone() },
            })?;
        Ok(EnableReport {
            name: name.clone(),
            put,
            replaced,
        })
    }
}

/// An enable that wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum EnableError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error("no plugin named {name} is installed")]
    NotInstalled { name: PluginName },
    #[error("the {name} plugin's manifest is refused: {source}")]
    Manifest {
        name: PluginName,
        source: ManifestError,
    },
    #[error(
        "the {name} plugin ships {record_type}, whose path in the board, {}, is too long; shorten the type's name",
        record_type.schema_path()
    )]
    Schema {
        name: PluginName,
        record_type: RecordType,
    },
    #[error(
        "the board changed while the {name} plugin was being enabled, so nothing was written; run the enable again"
    )]
    Changed { name: PluginName },
    #[error(transparent)]
    Put(#[from] PutError),
}

impl EnableError {
    pub fn code(&self) -> Code {
        match self {
            Self::Discovery(error) => error.code(),
            Self::NotInstalled { .. } => Code::PluginNotInstalled,
            Self::Manifest { source, .. } => source.code(),
            Self::Schema { .. } => Code::PluginBadManifest,
            Self::Changed { .. } => Code::PutStale,
            Self::Put(error) => error.code(),
        }
    }
}

impl From<WriteError> for EnableError {
    fn from(error: WriteError) -> Self {
        Self::Put(error.into())
    }
}

#[cfg(test)]
mod tests;

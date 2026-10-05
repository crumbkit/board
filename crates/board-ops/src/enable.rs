//! `plugin enable`: adding a plugin to a board with the schemas it ships
//! (docs/plugins.md, Installing and enabling).

use std::path::{Path, PathBuf};

use board_model::{Code, LogicalPath, RecordType, canonical_json, parse_json};
use board_plugin::{DiscoveryError, ManifestError, PluginName, installed, shipped_schema_path};
use board_tree::WriteError;
use serde_json::Value;

use crate::Board;
use crate::board::config_file;
use crate::put::{Admitted, Expect, Failure, Prepared, PutError, PutReport};

/// What `plugin enable` wrote: each schema the board lacked, then
/// `.board/board.json` when it did not name the plugin.
#[derive(Debug)]
pub struct EnableReport {
    pub name: PluginName,
    pub put: PutReport,
}

impl Board {
    /// Enables the plugin `name` installed in `plugins`: writes each schema
    /// it ships to `.board/types/`, then adds it to `plugins` in
    /// `.board/board.json`, as one put. A schema the board already holds is
    /// left as it is; a different one refuses the enable, which then writes
    /// nothing. As with `put`, a kept schema is compared before the lock is
    /// taken, so a change to it while the enable runs goes unnoticed.
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
        let mut prepared = Vec::new();
        for (record_type, shipped) in &found.schemas {
            match self.held(record_type, shipped)? {
                Held::Absent => {
                    let physical = record_type.schema_path();
                    let stem = physical.strip_suffix(".json").unwrap_or(&physical);
                    let path: LogicalPath =
                        format!("/{stem}")
                            .parse()
                            .map_err(|_| EnableError::Schema {
                                name: name.clone(),
                                record_type: record_type.clone(),
                            })?;
                    prepared.push(self.schema(path, shipped)?);
                }
                Held::Same => {}
                Held::Other => {
                    return Err(EnableError::SchemaDiffers {
                        path: record_type.schema_path(),
                        shipped: found.directory.join(shipped_schema_path(record_type)),
                    });
                }
            }
        }
        // The configuration is read again here, so a plugin another process
        // enabled since the board was opened is kept.
        let changed = || EnableError::Changed { name: name.clone() };
        let (blob, mut config, current) = config_file(self.repository()).ok_or_else(changed)?;
        if !current.plugins.contains(name) {
            let fields = config.as_object_mut().ok_or_else(changed)?;
            let mut enabled = current.plugins;
            enabled.push(name.clone());
            fields.insert("plugins".to_owned(), serde_json::json!(enabled));
            let path = "/.board/board".parse().map_err(|_| changed())?;
            prepared.push(self.prepare(&path, Expect::Blob(blob), &canonical_json(&config))?);
        }
        if prepared.is_empty() {
            return Ok(EnableReport {
                name: name.clone(),
                put: PutReport {
                    writes: Vec::new(),
                    commit: None,
                },
            });
        }
        let put = self
            .write_all(&prepared, &format!("Enable the {name} plugin"))
            .map_err(|failure| match failure {
                Failure::Error(error) => EnableError::Put(error),
                Failure::Stale(_) => EnableError::Changed { name: name.clone() },
            })?;
        Ok(EnableReport {
            name: name.clone(),
            put,
        })
    }

    /// How the working tree holds `shipped`, the schema a plugin ships for
    /// `record_type`.
    pub(crate) fn held(
        &self,
        record_type: &RecordType,
        shipped: &Value,
    ) -> Result<Held, WriteError> {
        Ok(
            match self
                .repository()
                .read_working_file(&record_type.schema_path())?
            {
                None => Held::Absent,
                Some(bytes) if parse_json(&bytes).ok().as_ref() == Some(shipped) => Held::Same,
                Some(_) => Held::Other,
            },
        )
    }

    /// A shipped schema, to be written where nothing is.
    fn schema(&self, path: LogicalPath, schema: &Value) -> Result<Prepared, EnableError> {
        let target = self.repository().target(&path, false)?;
        Ok(Prepared {
            path,
            expect: Expect::Absent,
            canonical: canonical_json(schema),
            target,
            admitted: Admitted {
                value: schema.clone(),
                pointer: false,
                references: Vec::new(),
            },
        })
    }
}

/// How the board holds a schema a plugin ships.
pub(crate) enum Held {
    Absent,
    Same,
    Other,
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
    #[error("{path} holds a schema other than the one the plugin ships at {}", shipped.display())]
    SchemaDiffers { path: String, shipped: PathBuf },
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
            Self::SchemaDiffers { .. } => Code::PluginSchemaDiffers,
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

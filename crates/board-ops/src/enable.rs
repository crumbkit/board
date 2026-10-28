//! `plugin enable`: adding a plugin to a board with the schemas it ships
//! (docs/plugins.md, Installing and enabling).

use std::collections::HashMap;
use std::path::Path;

use board_model::{Code, RecordType, Source, TYPES_DIRECTORY, canonical_json};
use board_plugin::{DiscoveryError, ManifestError, PluginName, installed};
use board_tree::{LOCK_WAIT, WriteError};
use serde_json::Value;

use crate::Board;
use crate::board::config_file;
use crate::check::CheckError;
use crate::put::{Expect, Failure, PutError, PutReport, Removal, Schemas};
use crate::types::{Held, schema_at};

/// What `plugin enable` wrote: each schema the board lacked or held
/// otherwise, then `.board/board.json` when it did not name the plugin.
#[derive(Debug)]
pub struct EnableReport {
    pub name: PluginName,
    pub put: PutReport,
    /// Each schema the board held otherwise, by its path from the root.
    pub replaced: Vec<String>,
    /// Each schema in the plugin's namespace that it no longer ships and no
    /// record uses, which enable removed, by its path from the root.
    pub removed: Vec<String>,
    /// Each schema in the plugin's namespace that it no longer ships, which
    /// enable kept because records in the board's own tree still use it.
    pub kept: Vec<Kept>,
}

/// A schema the plugin no longer ships, kept for the records that use it.
#[derive(Debug, PartialEq, Eq)]
pub struct Kept {
    /// The schema's path from the root.
    pub schema: String,
    pub record_type: RecordType,
    /// How many records in the board's own tree have the type.
    pub records: u64,
}

impl Board {
    /// Enables the plugin `name` installed in `plugins`: writes each schema
    /// it ships to `.board/types/`, removes each schema in its namespace that
    /// it no longer ships, then adds it to `plugins` in `.board/board.json`,
    /// as one put. A schema the board already holds is left as it is; a
    /// different one is replaced, since the plugin's copy is the source. A
    /// schema to remove that a record in the working tree still names is
    /// kept, found through the index kept under `cache`, so the record stays
    /// valid until the plugin migrates it. The board is read under the lock
    /// the write holds.
    pub fn plugin_enable(
        &self,
        plugins: Option<&Path>,
        cache: Option<&Path>,
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
        let reader = &mut self.repository().reader();
        for (record_type, shipped) in &found.schemas {
            let expect = match Self::held(reader, record_type, shipped)? {
                Held::Same(_) | Held::Linked => continue,
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
        let (removed, kept) = self.unused(self.orphans(name, &found.schemas)?, cache)?;
        // The configuration is read again here, so a plugin another process
        // enabled since the board was opened is kept.
        let changed = || EnableError::Changed { name: name.clone() };
        let (blob, mut config, current) =
            config_file(self.repositories()).map_err(|_| changed())?;
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
        let removed_paths = removed.iter().map(|removal| removal.file.clone()).collect();
        if prepared.is_empty() && removed.is_empty() {
            return Ok(EnableReport {
                name: name.clone(),
                put: PutReport::new(Vec::new(), None),
                replaced,
                removed: removed_paths,
                kept,
            });
        }
        let message = format!("Enable the {name} plugin");
        let put = self
            .write_all(lock, &prepared, &removed, &message)
            .map_err(|failure| match failure {
                Failure::Error(error) => EnableError::Put(error),
                Failure::Stale(_) => EnableError::Changed { name: name.clone() },
            })?;
        Ok(EnableReport {
            name: name.clone(),
            put,
            replaced,
            removed: removed_paths,
            kept,
        })
    }

    /// Each schema file in the namespace of the plugin `name` whose type it
    /// does not ship among `shipped`, with that type. A symlink is left alone, since no put
    /// writes through one.
    fn orphans(
        &self,
        name: &PluginName,
        shipped: &[(RecordType, Value)],
    ) -> Result<Vec<(Removal, RecordType)>, EnableError> {
        let directory = format!("{TYPES_DIRECTORY}{name}");
        let entries = (self.repository())
            .walk_under(&Source::WorkingTree, &directory)
            .map_err(WriteError::from)?;
        let orphan = |entry: board_tree::Entry| {
            let record_type = RecordType::of_schema(&entry.path)?;
            let blob = entry.blob.filter(|_| entry.link.is_none())?;
            let held = shipped.iter().any(|(shipped, _)| *shipped == record_type);
            let removal = Removal {
                path: entry.logical?,
                file: entry.path,
                blob,
            };
            (!held).then_some((removal, record_type))
        };
        Ok(entries.into_iter().filter_map(orphan).collect())
    }

    /// Splits `orphans` into the schemas no record in the working tree
    /// uses, and those it keeps for the records that do.
    fn unused(
        &self,
        orphans: Vec<(Removal, RecordType)>,
        cache: Option<&Path>,
    ) -> Result<(Vec<Removal>, Vec<Kept>), EnableError> {
        if orphans.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let quoted: Vec<String> = orphans.iter().map(|(_, t)| format!("'{t}'")).collect();
        // A record type holds only lowercase letters, digits, `-` and `/`,
        // so it needs no escaping inside quotes.
        let sql = format!(
            "SELECT type, count(*) FROM records \
             WHERE source = 'worktree' AND type IN ({}) GROUP BY type",
            quoted.join(", ")
        );
        let index = self.index(cache).map_err(CheckError::from)?;
        let view = self.view(self.snapshot(index, None)?.decided);
        let rows = (view.and_then(|view| view.query(&sql, orphans.len(), None)))
            .map_err(CheckError::from)?
            .rows;
        let used: HashMap<&str, u64> = (rows.iter())
            .filter_map(|row| Some((row[0].as_str()?, row[1].as_u64()?)))
            .collect();
        let (mut removed, mut kept) = (Vec::new(), Vec::new());
        for (removal, record_type) in orphans {
            match used.get(record_type.as_str()) {
                Some(&records) => kept.push(Kept {
                    schema: removal.file,
                    record_type,
                    records,
                }),
                None => removed.push(removal),
            }
        }
        Ok((removed, kept))
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
    Records(#[from] CheckError),
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
            Self::Records(error) => error.code(),
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

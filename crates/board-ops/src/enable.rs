//! `plugin enable`: adding a plugin to a board with the schemas it ships
//! (docs/plugins.md, Installing and enabling).

use std::fmt::Write as _;
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

/// How many records of a type the plugin no longer ships a refusal names.
const LISTED: usize = 5;

/// What `plugin enable` wrote: each schema the board lacked or held
/// otherwise, then `.board/board.json` when it did not name the plugin.
#[derive(Debug)]
pub struct EnableReport {
    pub name: PluginName,
    pub put: PutReport,
    /// Each schema the board held otherwise, by its path from the root.
    pub replaced: Vec<String>,
    /// Each schema in the plugin's namespace that it no longer ships, which
    /// enable removed, by its path from the root.
    pub removed: Vec<String>,
}

impl Board {
    /// Enables the plugin `name` installed in `plugins`: writes each schema
    /// it ships to `.board/types/`, removes each schema in its namespace that
    /// it no longer ships, then adds it to `plugins` in `.board/board.json`,
    /// as one put. A schema the board already holds is left as it is; a
    /// different one is replaced, since the plugin's copy is the source. A
    /// schema to remove that a record in the working tree still names is
    /// refused, found through the index kept under `cache`. The board is
    /// read under the lock the write holds.
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
        let removed = self.orphans(name, &found.schemas)?;
        if !removed.is_empty() {
            self.unused(name, &removed, cache)?;
        }
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
                put: PutReport {
                    writes: Vec::new(),
                    commit: None,
                },
                replaced,
                removed: removed_paths,
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
        })
    }

    /// Each schema file in the namespace of the plugin `name` whose type it
    /// does not ship among `shipped`. A symlink is left alone, since no put
    /// writes through one.
    fn orphans(
        &self,
        name: &PluginName,
        shipped: &[(RecordType, Value)],
    ) -> Result<Vec<Removal>, EnableError> {
        let directory = format!("{TYPES_DIRECTORY}{name}");
        let entries = (self.repository())
            .walk_under(&Source::WorkingTree, &directory)
            .map_err(WriteError::from)?;
        let orphan = |entry: board_tree::Entry| {
            let record_type = RecordType::of_schema(&entry.path)?;
            let blob = entry.blob.filter(|_| entry.link.is_none())?;
            let held = shipped.iter().any(|(shipped, _)| *shipped == record_type);
            (!held).then_some(Removal {
                path: entry.logical?,
                file: entry.path,
                blob,
            })
        };
        Ok(entries.into_iter().filter_map(orphan).collect())
    }

    /// Refuses when a record in the working tree has the type of any schema
    /// in `removed`.
    fn unused(
        &self,
        name: &PluginName,
        removed: &[Removal],
        cache: Option<&Path>,
    ) -> Result<(), EnableError> {
        let types: Vec<RecordType> = (removed.iter())
            .filter_map(|removal| RecordType::of_schema(&removal.file))
            .collect();
        let quoted: Vec<String> = types.iter().map(|t| format!("'{t}'")).collect();
        // A record type holds only lowercase letters, digits, `-` and `/`,
        // so it needs no escaping inside quotes.
        let sql = format!(
            "SELECT coalesce(logical, path), count(*) OVER () FROM records \
             WHERE source = 'worktree' AND type IN ({}) ORDER BY path",
            quoted.join(", ")
        );
        let index = self.index(cache).map_err(CheckError::from)?;
        let view = self.view(self.snapshot(index, None)?.decided);
        let rows = (view.and_then(|view| view.query(&sql, LISTED, None)))
            .map_err(CheckError::from)?
            .rows;
        let Some(total) = rows.first().and_then(|row| row[1].as_u64()) else {
            return Ok(());
        };
        let records = rows.iter().filter_map(|row| row[0].as_str());
        Err(EnableError::TypeInUse {
            name: name.clone(),
            types,
            records: records.map(str::to_owned).collect(),
            total: usize::try_from(total).unwrap_or(usize::MAX),
        })
    }
}

/// What a `TypeInUse` refusal says: the types, and up to [`LISTED`]
/// records with a count of the rest.
fn in_use(name: &PluginName, types: &[RecordType], records: &[String], total: usize) -> String {
    let quoted: Vec<String> = types.iter().map(|t| format!("'{t}'")).collect();
    let types: Vec<String> = types.iter().map(ToString::to_string).collect();
    let mut listed = records.join(", ");
    if total > records.len() {
        let _ = write!(
            listed,
            " and {} more, which `board query \"SELECT path FROM records WHERE type IN ({})\"` lists",
            total - records.len(),
            quoted.join(", ")
        );
    }
    format!(
        "the {name} plugin no longer ships {}, but {total} record(s) in the board still have one of those types: {listed}; nothing was written",
        types.join(", ")
    )
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
    #[error("{}", in_use(name, types, records, *total))]
    TypeInUse {
        name: PluginName,
        types: Vec<RecordType>,
        records: Vec<String>,
        total: usize,
    },
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
            Self::TypeInUse { .. } => Code::PluginTypeInUse,
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

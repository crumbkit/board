//! Finding the plugins installed on this machine (docs/plugins.md, Installing
//! and enabling).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use board_model::{Code, MAX_RECORD_BYTES, RecordType, parse_json};
use board_validate::Schema;
use serde_json::Value;

use crate::manifest::{Manifest, ManifestError};

pub const MANIFEST_FILE: &str = "plugin.json";

/// A plugin Board can call: an entry of the plugins directory, or one built
/// into Board.
#[derive(Debug)]
pub struct Installed {
    /// The entry's name, which a manifest must repeat.
    pub name: String,
    pub origin: Origin,
    pub manifest: Result<Manifest, ManifestError>,
    /// Each type the manifest declares and the schema the plugin ships for
    /// it, in the manifest's order; empty when the manifest is refused.
    pub schemas: Vec<(RecordType, Value)>,
}

/// Where a plugin's code is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The plugin's directory, whose `run` Board starts.
    Directory(PathBuf),
    /// Built into Board: its place in the caller's list of built-in
    /// plugins, and the directory of an installed plugin of the same name.
    BuiltIn {
        index: usize,
        shadows: Option<PathBuf>,
    },
}

/// The plugin built into Board at `index`, with `manifest` and the schema
/// it ships for each type, each compiled. A declared type without one
/// makes the manifest refused.
pub fn built_in(index: usize, manifest: Manifest, schemas: &[(RecordType, Value)]) -> Installed {
    let compiled = manifest
        .types
        .iter()
        .map(|record_type| {
            let path = PathBuf::from(shipped_schema_path(record_type));
            let (_, schema) = schemas
                .iter()
                .find(|(shipped, _)| shipped == record_type)
                .ok_or_else(|| ManifestError::ShippedMissing {
                    record_type: record_type.clone(),
                    path: path.clone(),
                })?;
            compiled(record_type, path, schema.clone())
        })
        .collect();
    let name = manifest.name.to_string();
    let (manifest, schemas) = match compiled {
        Ok(schemas) => (Ok(manifest), schemas),
        Err(error) => (Err(error), Vec::new()),
    };
    Installed {
        name,
        origin: Origin::BuiltIn {
            index,
            shadows: None,
        },
        manifest,
        schemas,
    }
}

/// Every entry of `plugins`, sorted by name. A directory that does not exist
/// holds no plugins.
pub fn installed(plugins: &Path) -> Result<Vec<Installed>, DiscoveryError> {
    let unreadable = |source| DiscoveryError::Unreadable {
        directory: plugins.to_path_buf(),
        source,
    };
    let entries = match plugins.read_dir() {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(unreadable(error)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let directory = entry.map_err(unreadable)?.path();
        let name = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let loaded = read_bounded(&directory.join(MANIFEST_FILE))
            .ok_or(ManifestError::Missing)
            .and_then(|bytes| Manifest::parse(&name, &bytes))
            .and_then(|manifest| {
                let schemas = shipped(&directory, &manifest)?;
                Ok((manifest, schemas))
            });
        let (manifest, schemas) = match loaded {
            Ok((manifest, schemas)) => (Ok(manifest), schemas),
            Err(error) => (Err(error), Vec::new()),
        };
        found.push(Installed {
            name,
            origin: Origin::Directory(directory),
            manifest,
            schemas,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Where an installed plugin keeps the schema of a type it ships, from its
/// directory: the layout of a board's `.board/types/`.
pub fn shipped_schema_path(record_type: &RecordType) -> String {
    format!("types/{}.schema.json", record_type.as_str())
}

/// The schema the plugin in `directory` ships for each of `manifest`'s types,
/// each parsed and compiled.
fn shipped(
    directory: &Path,
    manifest: &Manifest,
) -> Result<Vec<(RecordType, Value)>, ManifestError> {
    manifest
        .types
        .iter()
        .map(|record_type| {
            let path = directory.join(shipped_schema_path(record_type));
            let bytes = read_bounded(&path).ok_or_else(|| ManifestError::ShippedMissing {
                record_type: record_type.clone(),
                path: path.clone(),
            })?;
            let schema = parse_json(&bytes).map_err(|source| ManifestError::ShippedJson {
                record_type: record_type.clone(),
                path: path.clone(),
                source,
            })?;
            compiled(record_type, path, schema)
        })
        .collect()
}

/// `schema`, the one a plugin ships for `record_type` at `path`, once it
/// compiles.
fn compiled(
    record_type: &RecordType,
    path: PathBuf,
    schema: Value,
) -> Result<(RecordType, Value), ManifestError> {
    Schema::compile(schema.clone()).map_err(|source| ManifestError::ShippedSchema {
        record_type: record_type.clone(),
        path,
        source,
    })?;
    Ok((record_type.clone(), schema))
}

/// A file's bytes, one past the record bound so the parse sees one too
/// large.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// A plugins directory that cannot be listed.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("the plugins directory {directory:?} cannot be listed: {source}")]
    Unreadable {
        directory: PathBuf,
        source: std::io::Error,
    },
}

impl DiscoveryError {
    pub fn code(&self) -> Code {
        match self {
            Self::Unreadable { .. } => Code::PluginNotInstalled,
        }
    }
}

#[cfg(test)]
mod tests;

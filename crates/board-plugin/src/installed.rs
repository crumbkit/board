//! Finding the plugins installed on this machine (docs/plugins.md, Installing
//! and enabling).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use board_model::{Code, MAX_RECORD_BYTES};

use crate::manifest::{Manifest, ManifestError};

pub const MANIFEST_FILE: &str = "plugin.json";

/// One entry of the plugins directory.
#[derive(Debug)]
pub struct Installed {
    /// The entry's name, which a manifest must repeat.
    pub name: String,
    pub directory: PathBuf,
    pub manifest: Result<Manifest, ManifestError>,
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
        let manifest = read_manifest(&directory.join(MANIFEST_FILE))
            .ok_or(ManifestError::Missing)
            .and_then(|bytes| Manifest::parse(&name, &bytes));
        found.push(Installed {
            name,
            directory,
            manifest,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The manifest's bytes, one past the record bound so the parse sees one too
/// large.
fn read_manifest(path: &Path) -> Option<Vec<u8>> {
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
            Self::Unreadable { .. } => Code::PluginBadManifest,
        }
    }
}

#[cfg(test)]
mod tests;

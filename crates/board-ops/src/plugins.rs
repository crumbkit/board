//! `plugin list` and `plugin find` (docs/plugins.md, Finding plugins).

use std::path::{Path, PathBuf};

use board_model::{CapabilityName, Code, Fields, PluginName, RecordType, Refusal};
use board_plugin::{
    CallError, DiscoveryError, Environment, Installed, Limits, Manifest, ManifestError, MetaFilter,
    installed,
};
use board_tree::Reader;
use serde_json::Value;

use crate::Board;
use crate::types::Held;

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

/// A call that failed: the plugin's or Board's refusal.
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
}

impl CallFailed {
    /// This failure as Board reports it: Board's code or the
    /// plugin's own, with the end of the plugin's stderr when it ran.
    pub fn refusal(&self) -> Refusal {
        match self {
            Self::Schema {
                plugin, problem, ..
            } => Refusal::new(
                problem.code,
                self,
                &Fields {
                    plugin: Some(plugin.clone()),
                    record_type: Some(problem.record_type.clone()),
                    schema: Some(problem.path.clone()),
                    ..Fields::default()
                },
            ),
            Self::Call { source } => source.refusal(),
        }
    }
}

/// Every installed plugin, and every enabled one that is not installed, in
/// name order.
#[derive(Debug)]
pub struct PluginListReport {
    pub plugins: Vec<Plugin>,
}

impl PluginListReport {
    /// Whether any plugin is broken or missing.
    pub fn degraded(&self) -> bool {
        self.plugins.iter().any(|plugin| plugin.code().is_some())
    }
}

#[derive(Debug)]
pub struct Plugin {
    /// The plugins directory's entry, or the name the board enables.
    pub name: String,
    /// Whether the board's configuration names it.
    pub enabled: bool,
    pub state: PluginState,
    /// Each schema an enabled plugin ships that the board lacks, holds
    /// differently or cannot read, in the manifest's order.
    pub schemas: Vec<SchemaProblem>,
}

/// A schema the board should hold for an enabled plugin.
#[derive(Debug)]
pub struct SchemaProblem {
    pub record_type: RecordType,
    /// Where the board keeps it, from the root.
    pub path: String,
    pub code: Code,
}

#[derive(Debug)]
pub enum PluginState {
    Installed(Manifest),
    BadManifest(ManifestError),
    NotInstalled,
}

impl Plugin {
    pub fn code(&self) -> Option<Code> {
        match &self.state {
            PluginState::Installed(_) => self.schemas.first().map(|problem| problem.code),
            PluginState::BadManifest(error) => Some(error.code()),
            PluginState::NotInstalled => Some(Code::PluginNotInstalled),
        }
    }
}

/// The installed plugins offering a matching capability, each holding only
/// what matched, in name order.
#[derive(Debug)]
pub struct PluginFindReport {
    pub plugins: Vec<Found>,
}

#[derive(Debug)]
pub struct Found {
    pub manifest: Manifest,
    pub enabled: bool,
}

impl Board {
    /// The plugins in `plugins`, a plugins directory; `None` when there is
    /// none, as without a home directory.
    pub fn plugin_list(&self, plugins: Option<&Path>) -> Result<PluginListReport, DiscoveryError> {
        let enabled = &self.config().plugins;
        let installed = match plugins {
            Some(plugins) => installed(plugins)?,
            None => Vec::new(),
        };
        let reader = &mut self.repository().reader();
        let mut listed: Vec<Plugin> = installed
            .into_iter()
            .map(|installed| {
                let enabled = enabled.iter().any(|name| name.as_str() == installed.name);
                let schemas = if enabled {
                    installed
                        .schemas
                        .iter()
                        .filter_map(|(record_type, shipped)| {
                            Self::problem(reader, record_type, shipped)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                Plugin {
                    enabled,
                    name: installed.name,
                    state: match installed.manifest {
                        Ok(manifest) => PluginState::Installed(manifest),
                        Err(error) => PluginState::BadManifest(error),
                    },
                    schemas,
                }
            })
            .collect();
        let missing: Vec<&PluginName> = enabled
            .iter()
            .filter(|name| !listed.iter().any(|plugin| plugin.name == name.as_str()))
            .collect();
        listed.extend(missing.into_iter().map(|name| Plugin {
            name: name.to_string(),
            enabled: true,
            state: PluginState::NotInstalled,
            schemas: Vec::new(),
        }));
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(PluginListReport { plugins: listed })
    }

    /// What is wrong with how the board holds `shipped`, the schema a
    /// plugin ships for `record_type`, as `plugin enable` would find it.
    pub(crate) fn problem(
        reader: &mut Reader<'_>,
        record_type: &RecordType,
        shipped: &Value,
    ) -> Option<SchemaProblem> {
        let code = match Self::held(reader, record_type, shipped) {
            Ok(Held::Same(_) | Held::Linked) => return None,
            Ok(Held::Absent) => Code::PluginSchemaMissing,
            Ok(Held::Other | Held::Unparsed(_)) => Code::PluginSchemaDiffers,
            Err(error) => error.code(),
        };
        Some(SchemaProblem {
            record_type: record_type.clone(),
            path: record_type.schema_path(),
            code,
        })
    }

    /// The installed plugins offering `capability`, when given, whose
    /// capabilities match every filter. A broken plugin offers nothing.
    pub fn plugin_find(
        &self,
        plugins: Option<&Path>,
        capability: Option<&CapabilityName>,
        filters: &[MetaFilter],
    ) -> Result<PluginFindReport, DiscoveryError> {
        let found = self
            .plugin_list(plugins)?
            .plugins
            .into_iter()
            .filter_map(|plugin| match plugin.state {
                PluginState::Installed(mut manifest) => {
                    manifest.retain(capability, filters);
                    (!manifest.capabilities.is_empty()).then_some(Found {
                        manifest,
                        enabled: plugin.enabled,
                    })
                }
                PluginState::BadManifest(_) | PluginState::NotInstalled => None,
            })
            .collect();
        Ok(PluginFindReport { plugins: found })
    }
}

#[cfg(test)]
mod tests;

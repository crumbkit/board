//! `plugin list` and `plugin find` (docs/plugins.md, Finding plugins).

use std::path::{Path, PathBuf};

use board_model::{CapabilityName, Code, Fields, PluginName, RecordType, Refusal};
use board_plugin::{
    CallError, DiscoveryError, Environment, Installed, Limits, Manifest, ManifestError, MetaFilter,
    Origin, built_in, installed,
};
use board_tree::Reader;
use serde_json::Value;

use crate::types::Held;
use crate::{Board, KnownBoards, Signing};

/// Where plugins are installed, what they inherit and what a run may take.
#[derive(Clone, Debug, Default)]
pub struct Plugins<'a> {
    pub directory: Option<&'a Path>,
    pub environment: Environment,
    pub limits: Limits,
    /// Where a built-in plugin's queries keep the index.
    pub cache: Option<&'a Path>,
    /// The known boards, which a built-in plugin's `--personal` and
    /// `decide` read as the plugin's own `board` would; none names none.
    pub known: Option<&'a KnownBoards>,
    /// How the owner's approval of a built-in plugin's decide is checked.
    pub signing: Signing,
    /// Whether a process plugin a nested call starts leads its own process
    /// group, as `board_plugin::Context::leads` says.
    pub leads: bool,
}

/// A call that failed: the plugin's or Board's refusal.
#[derive(Debug, thiserror::Error)]
pub enum CallFailed {
    #[error(
        "the {plugin} plugin ships {} {}, and the board does not hold the same schema at {}",
        problem.record_type,
        shipped.as_ref().map_or_else(|| "built into Board".to_owned(), |shipped| format!("at {}", shipped.display())),
        problem.path
    )]
    Schema {
        plugin: PluginName,
        shipped: Option<PathBuf>,
        problem: SchemaProblem,
    },
    #[error("{}", crate::checkout::Refused::ProcessPlugin(.plugin).refusal().message)]
    AtCheckout { plugin: PluginName },
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
            Self::AtCheckout { plugin } => {
                *crate::checkout::Refused::ProcessPlugin(plugin).refusal()
            }
            Self::Call { source } => source.refusal(),
        }
    }
}

/// Every built-in and installed plugin, and every enabled one that is
/// neither, in name order.
#[derive(Debug)]
pub struct PluginListReport {
    pub plugins: Vec<PluginEntry>,
}

impl PluginListReport {
    /// Whether any plugin is broken or missing.
    pub fn degraded(&self) -> bool {
        self.plugins.iter().any(|plugin| plugin.code().is_some())
    }
}

#[derive(Debug)]
pub struct PluginEntry {
    /// The plugins directory's entry, or the name the board enables.
    pub name: String,
    /// The installed plugin that shares a built-in one's name.
    pub shadows: Option<PathBuf>,
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

impl PluginEntry {
    pub fn code(&self) -> Option<Code> {
        match &self.state {
            PluginState::Installed(_) if self.shadows.is_some() => Some(Code::PluginDuplicate),
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
    /// The plugins built into Board, then those in `plugins`, a plugins
    /// directory, in name order. An installed plugin named like a built-in
    /// one is left out, and the built-in one records its directory.
    pub(crate) fn offered(&self, plugins: Option<&Path>) -> Result<Vec<Installed>, DiscoveryError> {
        let mut offered: Vec<Installed> = (self.built_in.0.iter().enumerate())
            .map(|(index, plugin)| built_in(index, plugin.manifest(), &plugin.schemas()))
            .collect();
        let installed = match plugins {
            Some(plugins) => installed(plugins)?,
            None => Vec::new(),
        };
        for plugin in installed {
            let same = offered.iter_mut().find(|same| same.name == plugin.name);
            match (same.map(|same| &mut same.origin), &plugin.origin) {
                (Some(Origin::BuiltIn { shadows, .. }), Origin::Directory(directory)) => {
                    *shadows = Some(directory.clone());
                }
                _ => offered.push(plugin),
            }
        }
        offered.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(offered)
    }

    /// The plugins built into Board and those in `plugins`, a plugins
    /// directory; `None` when there is none, as without a home directory.
    pub fn plugin_list(&self, plugins: Option<&Path>) -> Result<PluginListReport, DiscoveryError> {
        let enabled = &self.config().plugins;
        let installed = self.offered(plugins)?;
        let reader = &mut self.repository().reader();
        let mut listed: Vec<PluginEntry> = installed
            .into_iter()
            .map(|installed| {
                let enabled = match &installed.manifest {
                    Ok(manifest) => manifest.enabling(enabled).is_some(),
                    Err(_) => enabled.iter().any(|name| name.as_str() == installed.name),
                };
                let schemas = if enabled {
                    self.problems(reader, &installed.origin, &installed.schemas)
                } else {
                    Vec::new()
                };
                PluginEntry {
                    enabled,
                    name: installed.name,
                    shadows: installed.origin.shadows().map(Path::to_path_buf),
                    state: match installed.manifest {
                        Ok(manifest) => PluginState::Installed(manifest),
                        Err(error) => PluginState::BadManifest(error),
                    },
                    schemas,
                }
            })
            .collect();
        // A former name an installed plugin lists is that plugin, not a missing one.
        let reached = |plugin: &PluginEntry, name: &PluginName| {
            plugin.name == name.as_str()
                || matches!(&plugin.state, PluginState::Installed(manifest) if manifest.former_names.contains(name))
        };
        let missing: Vec<&PluginName> = enabled
            .iter()
            .filter(|name| !listed.iter().any(|plugin| reached(plugin, name)))
            .collect();
        listed.extend(missing.into_iter().map(|name| PluginEntry {
            name: name.to_string(),
            shadows: None,
            enabled: true,
            state: PluginState::NotInstalled,
            schemas: Vec::new(),
        }));
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(PluginListReport { plugins: listed })
    }

    /// What is wrong with how the board holds each of `schemas`, those the
    /// plugin at `origin` ships, as [`Board::problem`] finds it. A schema
    /// the board lacks is no problem when it holds the other type of the
    /// pair a built-in plugin's [`crate::Plugin::renamed_types`] names.
    pub(crate) fn problems(
        &self,
        reader: &mut Reader<'_>,
        origin: &Origin,
        schemas: &[(RecordType, Value)],
    ) -> Vec<SchemaProblem> {
        let renamed = match origin {
            Origin::BuiltIn { index, .. } => {
                (self.built_in.0.get(*index)).map_or(&[][..], |plugin| plugin.renamed_types())
            }
            Origin::Directory(_) => &[],
        };
        let problems: Vec<SchemaProblem> = (schemas.iter())
            .filter_map(|(record_type, shipped)| Self::problem(reader, record_type, shipped))
            .collect();
        let held = |name: &str| {
            !problems
                .iter()
                .any(|problem| problem.record_type.as_str() == name)
        };
        let twin = |name: &str| {
            (renamed.iter()).find_map(|&(old, new)| match name {
                _ if name == old => Some(new),
                _ if name == new => Some(old),
                _ => None,
            })
        };
        let excused: Vec<bool> = (problems.iter())
            .map(|problem| {
                problem.code == Code::PluginSchemaMissing
                    && twin(problem.record_type.as_str()).is_some_and(held)
            })
            .collect();
        (problems.into_iter().zip(excused))
            .filter_map(|(problem, excused)| (!excused).then_some(problem))
            .collect()
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
            Ok(Held::Annotated | Held::Other | Held::Unparsed(_)) => Code::PluginSchemaDiffers,
            Err(error) => error.code(),
        };
        Some(SchemaProblem {
            record_type: record_type.clone(),
            path: record_type.schema_path(),
            code,
        })
    }

    /// The plugins offering `capability`, when given, whose capabilities
    /// match every filter. A broken plugin, and a built-in one installed
    /// again, offers nothing.
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
            .filter(|plugin| plugin.shadows.is_none())
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

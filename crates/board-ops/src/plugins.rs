//! `plugin list` and `plugin find` (docs/plugins.md, Finding plugins).

use std::path::Path;

use board_model::Code;
use board_plugin::{
    CapabilityName, DiscoveryError, Manifest, ManifestError, MetaFilter, PluginName, installed,
};

use crate::Board;

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
            PluginState::Installed(_) => None,
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
        let mut listed: Vec<Plugin> = installed
            .into_iter()
            .map(|installed| Plugin {
                enabled: enabled.iter().any(|name| name.as_str() == installed.name),
                name: installed.name,
                state: match installed.manifest {
                    Ok(manifest) => PluginState::Installed(manifest),
                    Err(error) => PluginState::BadManifest(error),
                },
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
        }));
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(PluginListReport { plugins: listed })
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

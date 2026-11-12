//! Plugin commands: the words that run a plugin's capabilities as
//! `board <plugin> <capability>` (docs/plugins.md, Plugin commands).

use std::path::Path;

use board_plugin::{DiscoveryError, Installed, Manifest};

use crate::Board;

impl Board {
    /// The manifest of each plugin this board enables, whose name and alias
    /// run its capabilities, from the plugins built into Board and those in
    /// `plugins`, in name order. A broken install, or an installed copy of a
    /// built-in plugin, offers no command.
    pub fn aliases(&self, plugins: Option<&Path>) -> Result<Vec<Manifest>, DiscoveryError> {
        Ok(self.aliased(self.offered(plugins)?))
    }

    /// The manifests among `offered` that [`Board::aliases`] keeps.
    pub(crate) fn aliased(&self, offered: Vec<Installed>) -> Vec<Manifest> {
        let enabled = &self.config().plugins;
        (offered.into_iter())
            .filter(|installed| installed.origin.shadows().is_none())
            .filter_map(|installed| installed.manifest.ok())
            .filter(|manifest| manifest.enabling(enabled).is_some())
            .collect()
    }
}

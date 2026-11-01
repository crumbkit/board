//! Plugin aliases: the word that runs a plugin's capabilities as
//! `board <alias> <capability>` (docs/plugins.md, Aliases).

use std::path::Path;

use board_plugin::{DiscoveryError, Installed, Manifest};

use crate::Board;

impl Board {
    /// The manifest of each plugin this board enables that declares an
    /// alias, from the plugins built into Board and those in `plugins`, in
    /// name order. A broken install, or an installed copy of a built-in
    /// plugin, offers no alias.
    pub fn aliases(&self, plugins: Option<&Path>) -> Result<Vec<Manifest>, DiscoveryError> {
        Ok(self.aliased(self.offered(plugins)?))
    }

    /// The manifests among `offered` that [`Board::aliases`] keeps.
    pub(crate) fn aliased(&self, offered: Vec<Installed>) -> Vec<Manifest> {
        let enabled = &self.config().plugins;
        (offered.into_iter())
            .filter(|installed| installed.origin.shadows().is_none())
            .filter(|installed| enabled.iter().any(|name| name.as_str() == installed.name))
            .filter_map(|installed| installed.manifest.ok())
            .filter(|manifest| manifest.alias.is_some())
            .collect()
    }
}

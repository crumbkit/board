//! Plugin aliases: the word that runs a plugin's capabilities as
//! `board <alias> <capability>` (docs/plugins.md, Aliases).

use std::fmt;
use std::path::Path;

use board_model::{Code, Fields, PluginName, Refusal};
use board_plugin::{Alias, DiscoveryError, Installed, Manifest};

use crate::Board;

/// What an alias collides with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Collision {
    /// One of Board's built-in commands, which always runs instead.
    Command(String),
    /// Another enabled plugin, whose alias is the same word.
    Plugin(PluginName),
}

impl fmt::Display for Collision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(command) => write!(formatter, "Board's built-in command {command}"),
            Self::Plugin(plugin) => write!(
                formatter,
                "the alias of the {plugin} plugin, which the board enables"
            ),
        }
    }
}

/// A plugin's alias that names something else as well.
#[derive(Debug, thiserror::Error)]
#[error("the {plugin} plugin's alias {alias} is also {with}")]
pub struct AliasConflict {
    pub plugin: PluginName,
    pub alias: Alias,
    pub with: Collision,
}

impl AliasConflict {
    pub fn refusal(&self) -> Refusal {
        let fields = Fields {
            plugin: Some(self.plugin.clone()),
            ..Fields::default()
        };
        Refusal::new(Code::PluginAliasConflict, self, &fields)
    }
}

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

/// How `manifest`'s alias collides with one of `commands`, Board's built-in
/// commands, or else with the alias of another plugin in `aliased`.
pub fn collision(
    manifest: &Manifest,
    commands: &[&str],
    aliased: &[Manifest],
) -> Option<AliasConflict> {
    let alias = manifest.alias.as_ref()?;
    let with = match commands.iter().find(|command| **command == alias.as_str()) {
        Some(command) => Collision::Command((*command).to_owned()),
        None => Collision::Plugin(
            (aliased.iter())
                .find(|other| other.name != manifest.name && other.alias.as_ref() == Some(alias))?
                .name
                .clone(),
        ),
    };
    Some(AliasConflict {
        plugin: manifest.name.clone(),
        alias: alias.clone(),
        with,
    })
}

//! `call` (docs/plugins.md, Calling a capability).

use std::path::Path;

use board_index::PluginDatabase;
use board_plugin::{CallError, CallReport, Context, Environment, Limits, Target};

use crate::Board;

impl Board {
    /// Runs the capability `target` names with `input`, in a plugin from
    /// `plugins` that this board enables.
    pub fn call(
        &self,
        target: &Target,
        input: &[u8],
        plugins: Option<&Path>,
        environment: &Environment,
        cache: Option<&Path>,
        limits: Limits,
    ) -> Result<CallReport, CallError> {
        environment.depth()?;
        let installed = match plugins {
            Some(plugins) => board_plugin::installed(plugins)
                .map_err(|source| CallError::Discovery { source })?,
            None => Vec::new(),
        };
        let chosen = board_plugin::choose(installed, &self.config().plugins, target)?;
        let root = self.repository().path();
        let manifest = &chosen.manifest;
        let database = PluginDatabase::open(
            cache,
            root,
            &format!("{}@{}.sqlite", manifest.name, manifest.version),
        )
        .map_err(|source| CallError::Database {
            plugin: manifest.name.clone(),
            source,
        })?;
        board_plugin::invoke(
            &chosen,
            input,
            &Context {
                root,
                database: database.path(),
                environment,
                limits,
            },
        )
    }
}

#[cfg(test)]
mod tests;

//! `call` (docs/plugins.md, Calling a capability).

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::SystemTime;

use board_plugin::{
    CallError, CallReport, Chosen, Context, Origin, PluginFailure, Target, shipped_schema_path,
};
use serde_json::Value;

use crate::Board;
use crate::host::{HostBoard, Hosted, PluginError, Settings, Variables};
use crate::plugins::{CallFailed, Plugins};

impl Board {
    /// Runs the capability `target` names with `input`, in a plugin built
    /// into Board or installed, that this board enables and whose shipped
    /// schemas it holds. An installed plugin reads the board through its
    /// own `board query`; a built-in one through its host.
    pub fn call(
        &self,
        target: &Target,
        input: &[u8],
        plugins: &Plugins<'_>,
    ) -> Result<CallReport, CallFailed> {
        plugins.environment.depth()?;
        let offered = self
            .offered(plugins.directory)
            .map_err(|source| CallError::Discovery { source })?;
        let chosen = board_plugin::choose(offered, &self.config().plugins, target)?;
        self.trace.borrow_mut().capability = Some(format!(
            "{}.{}",
            chosen.manifest.name,
            chosen.capability().name
        ));
        if let Some(directory) = chosen.origin.shadows() {
            return Err(CallError::Duplicate {
                plugin: chosen.manifest.name.clone(),
                directory: directory.to_path_buf(),
            }
            .into());
        }
        // A record Board admits must be one the plugin's own schema admits.
        let reader = &mut self.repository().reader();
        if let Some(problem) = chosen
            .schemas
            .iter()
            .find_map(|(record_type, shipped)| Self::problem(reader, record_type, shipped))
        {
            return Err(CallFailed::Schema {
                plugin: chosen.manifest.name,
                shipped: match chosen.origin {
                    Origin::Directory(directory) => {
                        Some(directory.join(shipped_schema_path(&problem.record_type)))
                    }
                    Origin::BuiltIn { .. } => None,
                },
                problem,
            });
        }
        let _timed = self.timed("plugin.call");
        let input = board_plugin::input(&chosen, input);
        match &chosen.origin {
            Origin::Directory(directory) => {
                let spawned = RefCell::default();
                let report = input.and_then(|input| {
                    board_plugin::invoke(
                        &chosen,
                        directory,
                        &input,
                        &Context {
                            root: self.repository().path(),
                            environment: &plugins.environment,
                            limits: plugins.limits,
                            spawned: &spawned,
                            leads: plugins.leads,
                        },
                    )
                });
                self.trace.borrow_mut().started(spawned.take());
                Ok(report?)
            }
            Origin::BuiltIn { index, .. } => {
                self.trace.borrow_mut().started(Vec::new());
                Ok(self.hosted(&chosen, *index, input?, plugins)?)
            }
        }
    }

    /// Runs the chosen capability of the built-in plugin at `index` in this
    /// process, and checks what it returns as a process plugin's reply.
    fn hosted(
        &self,
        chosen: &Chosen,
        index: usize,
        input: Value,
        plugins: &Plugins<'_>,
    ) -> Result<CallReport, CallError> {
        let environment = &plugins.environment;
        let root = self.repository().path();
        let host = Hosted {
            board: HostBoard::Called(self),
            settings: Settings {
                now: SystemTime::now(),
                day_start: (environment.day_start.as_ref())
                    .map(|text| text.to_string_lossy().into_owned()),
                caller: environment.caller.as_ref().map(PathBuf::from),
                environment: Variables(board_plugin::passed(chosen, environment)?),
            },
            // As `board call` sets `BOARD_PLUGIN` for a child: the plugin
            // already named, else this one. A put refuses a name that does
            // not parse, as the child's `board put` would.
            plugin: match &environment.plugin {
                Some(text) => {
                    (text.to_str().and_then(|name| name.parse().ok())).ok_or_else(|| text.clone())
                }
                None => Ok(chosen.manifest.name.clone()),
            },
            // What the plugin's own `board` would read.
            plugins: Plugins {
                environment: environment.nested(chosen, root)?,
                known: plugins.known,
                signing: plugins.signing.clone(),
                // This Board leads its plugins' groups, and the built-in
                // runs in it, so a process plugin it calls leads its own.
                leads: plugins.leads || environment.depth()? == 0,
                ..*plugins
            },
            root,
        };
        let plugin = &chosen.manifest.name;
        let capability = &chosen.capability().name;
        let bad = |reason| CallError::BadOutput {
            plugin: plugin.clone(),
            reason,
            stderr: Vec::new(),
        };
        let output = match self.built_in.0[index].call(&host, capability, input) {
            Ok(output) => board_plugin::output(chosen, output).map_err(bad)?,
            Err(PluginError {
                code,
                message,
                fields,
                hint,
            }) => {
                return Err(CallError::Plugin {
                    plugin: plugin.clone(),
                    failure: Box::new(
                        PluginFailure::parse(plugin, code, message, fields, hint).map_err(bad)?,
                    ),
                    stderr: Vec::new(),
                });
            }
        };
        Ok(CallReport {
            plugin: plugin.clone(),
            capability: capability.clone(),
            output,
        })
    }
}

#[cfg(test)]
mod tests;

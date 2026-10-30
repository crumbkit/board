//! `call` (docs/plugins.md, Calling a capability).

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::SystemTime;

use board_model::{PluginCode, PluginCodeError};
use board_plugin::{
    BadOutput, CallError, CallReport, Chosen, Context, Origin, PluginFailure, Target,
    shipped_schema_path,
};
use serde_json::Value;

use crate::Board;
use crate::host::{Hosted, PluginError, Settings};
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
        if let Origin::BuiltIn {
            shadows: Some(directory),
            ..
        } = &chosen.origin
        {
            return Err(CallFailed::Duplicate {
                plugin: chosen.manifest.name,
                directory: directory.clone(),
            });
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
        let input = board_plugin::input(&chosen, input)?;
        match &chosen.origin {
            Origin::Directory(directory) => {
                let spawned = RefCell::default();
                let report = board_plugin::invoke(
                    &chosen,
                    directory,
                    &input,
                    &Context {
                        root: self.repository().path(),
                        environment: &plugins.environment,
                        limits: plugins.limits,
                        spawned: &spawned,
                    },
                );
                self.trace.borrow_mut().started(spawned.take());
                Ok(report?)
            }
            Origin::BuiltIn { index, .. } => {
                self.trace.borrow_mut().started(Vec::new());
                Ok(self.hosted(&chosen, *index, input, plugins)?)
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
        let host = Hosted {
            board: self,
            cache: plugins.cache,
            settings: Settings {
                now: SystemTime::now(),
                day_start: (environment.day_start.as_ref())
                    .map(|text| text.to_string_lossy().into_owned()),
                caller: environment.caller.as_ref().map(PathBuf::from),
            },
        };
        let plugin = chosen.manifest.name.clone();
        let capability = chosen.capability().name.clone();
        let bad = |reason| CallError::BadOutput {
            plugin: plugin.clone(),
            reason,
            stderr: Vec::new(),
        };
        let output = match self.built_in.0[index].call(&host, &capability, input) {
            Ok(output) => board_plugin::output(chosen, output).map_err(bad)?,
            Err(PluginError {
                code,
                message,
                fields,
            }) => {
                return Err(match PluginCode::parse(&plugin, code) {
                    Ok(code) => CallError::Plugin {
                        plugin: plugin.clone(),
                        failure: PluginFailure {
                            code,
                            message,
                            fields,
                        },
                        stderr: Vec::new(),
                    },
                    Err(PluginCodeError { code }) => bad(BadOutput::Code { code }),
                });
            }
        };
        Ok(CallReport {
            plugin,
            capability,
            output,
        })
    }
}

#[cfg(test)]
mod tests;

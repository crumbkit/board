//! `call` (docs/plugins.md, Calling a capability).

use std::cell::RefCell;

use board_plugin::{CallReport, Context, Target, shipped_schema_path};

use crate::Board;
use crate::plugins::{CallFailed, Plugins};

impl Board {
    /// Runs the capability `target` names with `input`, in an installed plugin
    /// that this board enables and whose shipped schemas it holds. The
    /// plugin reads the board through its own `board query`.
    pub fn call(
        &self,
        target: &Target,
        input: &[u8],
        plugins: &Plugins<'_>,
    ) -> Result<CallReport, CallFailed> {
        plugins.environment.depth()?;
        let chosen = board_plugin::choose(plugins.installed()?, &self.config().plugins, target)?;
        // A record Board admits must be one the plugin's own schema admits.
        let reader = &mut self.repository().reader();
        if let Some(problem) = chosen
            .schemas
            .iter()
            .find_map(|(record_type, shipped)| Self::problem(reader, record_type, shipped))
        {
            return Err(CallFailed::Schema {
                plugin: chosen.manifest.name,
                shipped: chosen
                    .directory
                    .join(shipped_schema_path(&problem.record_type)),
                problem,
            });
        }
        let _timed = self.timed("plugin.call");
        let spawned = RefCell::default();
        let report = board_plugin::invoke(
            &chosen,
            input,
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
}

#[cfg(test)]
mod tests;

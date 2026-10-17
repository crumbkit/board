//! `call` (docs/plugins.md, Calling a capability).

use std::cell::RefCell;

use board_plugin::{CallReport, Context, Target, shipped_schema_path};

use crate::Board;
use crate::extraction::{CallFailed, Plugins, database};

/// What a capability returned, and whether its plugin's database was a
/// temporary file though the index is kept.
#[derive(Debug)]
pub struct Called {
    pub report: CallReport,
    pub fallback: bool,
}

impl Board {
    /// Runs the capability `target` names with `input`, in an installed plugin
    /// that this board enables and whose shipped schemas it holds, with its
    /// database beside the index kept in `cache`. It extracts nothing: the
    /// plugin's own `board query` does.
    pub fn call(
        &self,
        target: &Target,
        input: &[u8],
        plugins: &Plugins<'_>,
        cache: Option<&std::path::Path>,
    ) -> Result<Called, CallFailed> {
        plugins.environment.depth()?;
        let chosen = board_plugin::choose(plugins.installed()?, &self.config().plugins, target)?;
        // A record Board admits must be one the plugin's own schema admits.
        if let Some(problem) = chosen
            .schemas
            .iter()
            .find_map(|(record_type, shipped)| self.problem(record_type, shipped))
        {
            return Err(CallFailed::Schema {
                plugin: chosen.manifest.name,
                shipped: chosen
                    .directory
                    .join(shipped_schema_path(&problem.record_type)),
                problem,
            });
        }
        let index = self.index(cache)?;
        let database = database(index.storage(), &chosen.manifest)?;
        // The plugin may run `board query` or `board call`, which open the
        // same index; holding it past here would make them wait and fall back.
        drop(index);
        let _timed = self.timed("plugin.call");
        let spawned = RefCell::default();
        let report = board_plugin::invoke(
            &chosen,
            input,
            &Context {
                root: self.repository().path(),
                database: database.path(),
                environment: &plugins.environment,
                limits: plugins.limits,
                spawned: &spawned,
            },
        );
        self.trace.borrow_mut().started(spawned.take());
        let report = report?;
        Ok(Called {
            report,
            fallback: database.fallback(),
        })
    }
}

#[cfg(test)]
mod tests;

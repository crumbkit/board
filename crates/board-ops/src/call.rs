//! `call` (docs/plugins.md, Calling a capability).

use board_index::Index;
use board_plugin::{CallReport, Context, Target};

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
    /// that this board enables, after extracting for it under the index kept
    /// in `cache`.
    pub fn call(
        &self,
        target: &Target,
        input: &[u8],
        plugins: &Plugins<'_>,
        cache: Option<&std::path::Path>,
    ) -> Result<Called, CallFailed> {
        plugins.environment.depth()?;
        let chosen = board_plugin::choose(plugins.installed()?, &self.config().plugins, target)?;
        let mut index = Index::open(cache, self.repository().path())?;
        let database = database(index.storage(), &chosen.manifest)?;
        if !chosen.manifest.extract.is_empty() {
            let snapshot = self.snapshot(&mut index, None)?;
            self.extract_into(
                &snapshot,
                &chosen.manifest,
                &chosen.directory,
                &database,
                plugins,
            )?;
        }
        // The plugin may run `board query` or `board call`, which open the
        // same index; holding it here would make them wait and fall back.
        drop(index);
        let report = board_plugin::invoke(
            &chosen,
            input,
            &Context {
                root: self.repository().path(),
                database: database.path(),
                environment: &plugins.environment,
                limits: plugins.limits,
            },
        )?;
        Ok(Called {
            report,
            fallback: database.fallback(),
        })
    }
}

#[cfg(test)]
mod tests;

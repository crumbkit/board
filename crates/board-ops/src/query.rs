//! `board query`: one read-only SQL statement over the index
//! (docs/architecture.md, The index).

use std::path::Path;

use board_index::{Fallback, Index, IndexError};
use board_model::Code;
use board_plugin::{Installed, PluginName};
use serde_json::Value;

use crate::board::Board;
use crate::check::CheckError;
use crate::extraction::{Plugins, database};

/// The most rows `query` returns, and the most entries a listing shows,
/// unless asked otherwise (STANDARDS.md 23).
pub const DEFAULT_LIMIT: usize = 100;
/// The most rows `query` returns at all, and the largest `--limit`.
pub const MAX_LIMIT: usize = 10_000;

/// What one statement returned.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryReport {
    pub columns: Vec<String>,
    /// Each row's values, in column order.
    pub rows: Vec<Vec<Value>>,
    /// Whether rows were left out to keep within the limit.
    pub truncated: bool,
    pub memory: Option<Fallback>,
    /// Each enabled plugin that could not extract.
    pub plugins: Vec<PluginFailed>,
    /// Each enabled plugin whose database was a temporary file though the
    /// index is kept.
    pub fallbacks: Vec<PluginName>,
}

/// An enabled plugin that could not extract: its code, Board's or its own,
/// and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFailed {
    pub plugin: PluginName,
    pub code: String,
    pub message: String,
}

impl Board {
    /// Refreshes the index kept under `cache` and decides every entry, as an
    /// unscoped check does, extracts for each enabled plugin and attaches its
    /// database as `p_<name>`, then runs `sql`, returning at most `limit`
    /// rows. A plugin that fails is reported and the query still runs.
    pub fn query(
        &self,
        sql: &str,
        limit: usize,
        cache: Option<&Path>,
        plugins: &Plugins<'_>,
    ) -> Result<QueryReport, QueryError> {
        let mut index = Index::open(cache, self.repository().path()).map_err(QueryError::from)?;
        let checked = self.check_in(&mut index, None)?;
        let mut report = QueryReport {
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
            memory: checked.memory,
            plugins: Vec::new(),
            fallbacks: Vec::new(),
        };
        // Held until the query ends, so a temporary database outlives it.
        let mut databases = Vec::new();
        let installed = plugins.installed();
        for name in &self.config().plugins {
            let failed = |code: &str, message: String| PluginFailed {
                plugin: name.clone(),
                code: code.to_owned(),
                message,
            };
            let found = match &installed {
                Ok(installed) => installed
                    .iter()
                    .find(|installed| installed.name == name.as_str()),
                Err(error) => {
                    report
                        .plugins
                        .push(failed(error.code_text(), error.to_string()));
                    continue;
                }
            };
            let (manifest, directory) = match found {
                Some(Installed {
                    manifest: Ok(manifest),
                    directory,
                    ..
                }) => (manifest, directory),
                Some(Installed {
                    manifest: Err(error),
                    ..
                }) => {
                    report
                        .plugins
                        .push(failed(error.code().as_str(), error.to_string()));
                    continue;
                }
                None => {
                    report.plugins.push(failed(
                        Code::PluginNotInstalled.as_str(),
                        format!("plugin {name} is enabled but not installed"),
                    ));
                    continue;
                }
            };
            let database = match database(index.storage(), manifest) {
                Ok(database) => database,
                Err(error) => {
                    report
                        .plugins
                        .push(failed(error.code_text(), error.to_string()));
                    continue;
                }
            };
            if let Err(error) = self.extract_into(&index, manifest, directory, &database, plugins) {
                report
                    .plugins
                    .push(failed(error.code_text(), error.to_string()));
            }
            if database.fallback() {
                report.fallbacks.push(name.clone());
            }
            let alias = format!("p_{}", name.as_str().replace('-', "_"));
            if let Err(error) = index.attach(&alias, database.path()) {
                report.plugins.push(failed(
                    Code::PluginFailed.as_str(),
                    format!(
                        "plugin database {} cannot be read, so {alias} is not attached; \
                         deleting it is safe: {error}",
                        database.path().display()
                    ),
                ));
            }
            databases.push(database);
        }
        let rows = index.query(sql, limit)?;
        report.columns = rows.columns;
        report.rows = rows.rows;
        report.truncated = rows.truncated;
        Ok(report)
    }
}

/// A query that could not run.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Check {
        #[from]
        source: CheckError,
    },
    #[error(transparent)]
    Index {
        #[from]
        source: IndexError,
    },
}

impl QueryError {
    pub fn code(&self) -> Code {
        match self {
            Self::Check { source } => source.code(),
            Self::Index { source } => source.code(),
        }
    }
}

#[cfg(test)]
mod tests;

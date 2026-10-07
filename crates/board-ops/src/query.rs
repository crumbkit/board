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

/// How many times a query runs while other processes prune under it.
const TRIES: usize = 3;

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
    /// rows. A plugin that fails is reported and the query still runs. When
    /// another process prunes what the views show before `sql` runs, it all
    /// runs again, three times in all.
    pub fn query(
        &self,
        sql: &str,
        limit: usize,
        cache: Option<&Path>,
        plugins: &Plugins<'_>,
    ) -> Result<QueryReport, QueryError> {
        let mut tries = 1;
        loop {
            match self.query_once(sql, limit, cache, plugins) {
                Err(QueryError::Index {
                    source: IndexError::Pruned,
                }) if tries < TRIES => tries += 1,
                result => return result,
            }
        }
    }

    fn query_once(
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
        let rows = index
            .query(sql, limit)
            .map_err(|error| unknown(&index, sql, error))?;
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
    /// The statement names a table or column the index does not have.
    #[error("{source}")]
    Unknown { source: IndexError, known: Known },
}

impl QueryError {
    pub fn code(&self) -> Code {
        match self {
            Self::Check { source } => source.code(),
            Self::Index { source } | Self::Unknown { source, .. } => source.code(),
        }
    }
}

/// The names a statement could have used instead of one that is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Known {
    /// Every table and view, and each attached plugin database.
    Tables(Vec<String>),
    /// Each table or view the statement names, with its columns; empty when
    /// it names none.
    Columns(Vec<(String, Vec<String>)>),
}

/// The names a statement can use: the views a run makes, the tables of the
/// cache, and each attached plugin database.
const TABLES: &str = "\
    SELECT name FROM sqlite_temp_schema WHERE type IN ('table', 'view') \
    UNION SELECT name FROM sqlite_schema WHERE type IN ('table', 'view') \
    UNION SELECT name FROM pragma_database_list WHERE name NOT IN ('main', 'temp') \
    ORDER BY name";

/// `error`, with the names there are when `SQLite` says the statement names a
/// table or column that is not there.
fn unknown(index: &Index, sql: &str, error: IndexError) -> QueryError {
    let message = error.to_string();
    let names = |sql: &str| -> Vec<String> {
        index.query(sql, MAX_LIMIT).map_or_else(
            |_| Vec::new(),
            |rows| {
                rows.rows
                    .into_iter()
                    .filter_map(|row| row.first()?.as_str().map(str::to_owned))
                    .collect()
            },
        )
    };
    let known = if message.contains("no such table: ") {
        Known::Tables(names(TABLES))
    } else if message.contains("no such column: ") {
        let words: Vec<String> = sql
            .split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .map(str::to_lowercase)
            .collect();
        Known::Columns(
            names(TABLES)
                .into_iter()
                .filter(|table| words.contains(table))
                .map(|table| {
                    let columns = names(&format!("SELECT name FROM pragma_table_info('{table}')"));
                    (table, columns)
                })
                .collect(),
        )
    } else {
        return error.into();
    };
    QueryError::Unknown {
        source: error,
        known,
    }
}

#[cfg(test)]
mod tests;

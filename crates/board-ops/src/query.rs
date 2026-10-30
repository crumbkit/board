//! `board query`: one read-only SQL statement over the index
//! (docs/architecture/index.md).

use std::path::Path;

use board_index::{Budget, Fallback, IndexError, View};
use board_model::{Code, Fields, Known, Refusal};
use serde_json::Value;

use crate::board::Board;
use crate::check::CheckError;

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
    /// Whether rows were left out to keep within the limit or the budget.
    pub truncated: bool,
    /// Whether the rows left out start with one that would have taken the
    /// rows past their budget.
    pub over_budget: bool,
    pub memory: Option<Fallback>,
}

impl Board {
    /// Refreshes the index kept under `cache` and decides every entry, as an
    /// unscoped check does, then runs `sql`, returning at most `limit` rows,
    /// within `budget` when there is one. When another process prunes what
    /// the views show before `sql` runs, it all runs again, three times in
    /// all.
    pub fn query(
        &self,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
        cache: Option<&Path>,
    ) -> Result<QueryReport, QueryError> {
        retried(|| {
            let (view, memory) = self.decided(cache)?;
            self.run(&view, memory, sql, limit, budget)
        })
    }

    /// The views of every entry, decided through the index kept under
    /// `cache`, and why that index is in memory when it is.
    pub(crate) fn decided(
        &self,
        cache: Option<&Path>,
    ) -> Result<(View, Option<Fallback>), QueryError> {
        let index = self.index(cache).map_err(QueryError::from)?;
        let snapshot = self.snapshot(index, None)?;
        let view = self.view(snapshot.decided)?;
        Ok((view, snapshot.report.memory))
    }

    /// Runs `sql` over `view`, returning at most `limit` rows, within `budget`
    /// when there is one.
    pub(crate) fn run(
        &self,
        view: &View,
        memory: Option<Fallback>,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
    ) -> Result<QueryReport, QueryError> {
        let timed = self.timed("query.sql");
        let rows = view
            .query(sql, limit, budget)
            .map_err(|error| unknown(view, sql, error))?;
        drop(timed);
        self.trace.borrow_mut().count("rows", rows.rows.len());
        Ok(QueryReport {
            columns: rows.columns,
            rows: rows.rows,
            truncated: rows.truncated,
            over_budget: rows.over_budget,
            memory,
        })
    }
}

/// Runs `once` again while another process prunes what its views show
/// before its statement runs, three times in all.
pub(crate) fn retried<T>(mut once: impl FnMut() -> Result<T, QueryError>) -> Result<T, QueryError> {
    let mut tries = 1;
    loop {
        match once() {
            Err(QueryError::Index {
                source: IndexError::Pruned,
            }) if tries < TRIES => tries += 1,
            result => return result,
        }
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

    pub fn refusal(&self) -> Refusal {
        let fields = Fields {
            known: match self {
                Self::Unknown { known, .. } => Some(known.clone()),
                Self::Check { .. } | Self::Index { .. } => None,
            },
            ..Fields::default()
        };
        Refusal::new(self.code(), self, &fields)
    }
}

/// The names a statement can use: the views a run makes and the tables of
/// the cache.
const TABLES: &str = "\
    SELECT name FROM sqlite_temp_schema WHERE type IN ('table', 'view') \
    UNION SELECT name FROM sqlite_schema WHERE type IN ('table', 'view') \
    ORDER BY name";

/// `error`, with the names there are when `SQLite` says the statement names a
/// table or column that is not there.
fn unknown(view: &View, sql: &str, error: IndexError) -> QueryError {
    let message = error.to_string();
    let names = |sql: &str| -> Vec<String> {
        view.query(sql, MAX_LIMIT, None).map_or_else(
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

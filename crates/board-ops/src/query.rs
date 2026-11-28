//! `board query`: one read-only SQL statement over the index
//! (docs/architecture/index.md).

use std::path::Path;
use std::time::{Duration, Instant};

use board_index::{Budget, Fallback, IndexError, View};
use board_model::{Code, Fields, Known, Refusal};
use serde_json::Value;

use crate::board::Board;
use crate::check::CheckError;
use crate::snapshot::Need;

/// The most rows `query` returns, and the most entries a listing shows,
/// unless asked otherwise (STANDARDS.md 23).
pub const DEFAULT_LIMIT: usize = 100;
/// The most rows `query` returns to a limit, and the largest `--limit`.
pub const MAX_LIMIT: usize = 10_000;

/// How long a statement an agent types, with `board query`, may run before
/// it is stopped. It bounds how long an agent waits for an answer.
pub(crate) const TYPED_QUERY_TIME: Duration = Duration::from_secs(1);
/// How long a statement a Board command runs for itself, such as a plugin's
/// query through the host, may run. It matches the time the board-changes
/// mod waits for a lookup, so an inner query never outlasts what waits on
/// it. A statement that finishes after more than [`TYPED_QUERY_TIME`] shows
/// in the trace as `query.slow`.
pub(crate) const INNER_QUERY_TIME: Duration = Duration::from_secs(5);

/// The hint of a statement a Board command ran for itself that ran past
/// [`INNER_QUERY_TIME`]. The agent wrote no SQL, so there is none to narrow.
pub(crate) const INNER_TIMEOUT_HINT: &str = "Board ran this query for the command itself, so \
    there is no SQL to change; run the command again, and if it fails the same way, report the \
    command and this message to Board's developers";

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
    /// within `budget` when there is one, and stopping it past 1 second, the
    /// limit for a statement an agent types. When another process prunes
    /// what the views show before `sql` runs, it all runs again, three times
    /// in all.
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
        let snapshot = self.snapshot(index, None, Need::View)?;
        let view = self.view(snapshot.decided)?;
        Ok((view, snapshot.report.memory))
    }

    /// Runs `sql` over `view`, returning at most `limit` rows, within `budget`
    /// when there is one, and stopping it past [`TYPED_QUERY_TIME`].
    pub(crate) fn run(
        &self,
        view: &View,
        memory: Option<Fallback>,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
    ) -> Result<QueryReport, QueryError> {
        self.run_within(view, memory, sql, limit, budget, TYPED_QUERY_TIME)
    }

    /// Runs `sql` as [`Board::run`] does, but stopping it past `time`. A
    /// statement that is [`slow`] is also timed as `query.slow`.
    pub(crate) fn run_within(
        &self,
        view: &View,
        memory: Option<Fallback>,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
        time: Duration,
    ) -> Result<QueryReport, QueryError> {
        let timed = self.timed("query.sql");
        let started = Instant::now();
        let rows = view.query(sql, limit, budget, time);
        let timed_out = matches!(rows, Err(IndexError::Timeout { .. }));
        if slow(time, started.elapsed(), timed_out) {
            self.trace.borrow_mut().add("query.slow", started);
        }
        let rows = rows.map_err(|error| unknown(view, sql, error, time))?;
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

/// Whether a statement allowed `time` that took `took` finished slowly: it
/// was allowed more than [`TYPED_QUERY_TIME`], ran longer than that, and was
/// not stopped at its limit.
pub(crate) fn slow(time: Duration, took: Duration, timed_out: bool) -> bool {
    time > TYPED_QUERY_TIME && took > TYPED_QUERY_TIME && !timed_out
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
        let refusal = Refusal::new(self.code(), self, &fields);
        match self {
            Self::Index {
                source: IndexError::Timeout { limit },
            } if *limit > TYPED_QUERY_TIME => refusal.with_hint(INNER_TIMEOUT_HINT),
            _ => refusal,
        }
    }
}

/// The names a statement can use: the views a run makes and the tables of
/// the cache.
const TABLES: &str = "\
    SELECT name FROM sqlite_temp_schema WHERE type IN ('table', 'view') \
    UNION SELECT name FROM sqlite_schema WHERE type IN ('table', 'view') \
    ORDER BY name";

/// `error`, with the names there are when `SQLite` says the statement names a
/// table or column that is not there, each lookup stopped past `time`, the
/// statement's own limit.
fn unknown(view: &View, sql: &str, error: IndexError, time: Duration) -> QueryError {
    let message = error.to_string();
    let names = |sql: &str| -> Vec<String> {
        view.query(sql, MAX_LIMIT, None, time).map_or_else(
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

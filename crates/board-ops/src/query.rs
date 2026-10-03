//! `board query`: one read-only SQL statement over the index
//! (docs/architecture.md, The index).

use std::path::Path;

use board_index::{Fallback, Index, IndexError};
use board_model::Code;
use serde_json::Value;

use crate::board::Board;
use crate::check::CheckError;

/// The most rows `query` returns unless asked for fewer.
pub const DEFAULT_LIMIT: usize = 500;
/// The most rows `query` returns at all.
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
}

impl Board {
    /// Refreshes the index kept under `cache` and decides every entry, as an
    /// unscoped check does, then runs `sql` against it, returning at most
    /// `limit` rows.
    pub fn query(
        &self,
        sql: &str,
        limit: usize,
        cache: Option<&Path>,
    ) -> Result<QueryReport, QueryError> {
        let mut index = Index::open(cache, self.repository().path()).map_err(QueryError::from)?;
        let checked = self.check_in(&mut index, None)?;
        let rows = index.query(sql, limit)?;
        Ok(QueryReport {
            columns: rows.columns,
            rows: rows.rows,
            truncated: rows.truncated,
            memory: checked.memory,
        })
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

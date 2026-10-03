//! Reading the index with SQL (docs/architecture.md, The index).

use rusqlite::ErrorCode;
use rusqlite::types::ValueRef;
use serde_json::Value;

use crate::open::{Index, IndexError, hex};

/// What one statement returned: at most the rows asked for, and whether
/// any were left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

impl Index {
    /// Runs one statement that cannot write, returning at most `limit` rows.
    pub fn query(&self, sql: &str, limit: usize) -> Result<Rows, IndexError> {
        self.connection.pragma_update(None, "query_only", true)?;
        let result = self.read(sql, limit).map_err(blame);
        self.connection.pragma_update(None, "query_only", false)?;
        result
    }

    fn read(&self, sql: &str, limit: usize) -> Result<Rows, rusqlite::Error> {
        // rusqlite refuses a string holding more than one statement.
        let mut statement = self.connection.prepare(sql)?;
        let columns: Vec<String> = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let width = columns.len();
        let mut rows = Vec::new();
        let mut cursor = statement.query([])?;
        let mut truncated = false;
        while let Some(row) = cursor.next()? {
            if rows.len() == limit {
                truncated = true;
                break;
            }
            rows.push(
                (0..width)
                    .map(|at| Ok(value(row.get_ref(at)?)))
                    .collect::<Result<Vec<Value>, rusqlite::Error>>()?,
            );
        }
        Ok(Rows {
            columns,
            rows,
            truncated,
        })
    }
}

/// Whose fault a failed statement is: the index's when `SQLite` could not
/// use its file or memory, the query's otherwise.
pub(crate) fn blame(source: rusqlite::Error) -> IndexError {
    match source.sqlite_error_code() {
        Some(
            ErrorCode::DatabaseBusy
            | ErrorCode::DatabaseLocked
            | ErrorCode::SystemIoFailure
            | ErrorCode::DatabaseCorrupt
            | ErrorCode::NotADatabase
            | ErrorCode::OutOfMemory
            | ErrorCode::CannotOpen,
        ) => IndexError::Sql { source },
        _ => IndexError::Query { source },
    }
}

/// A `SQLite` value as JSON. A blob, which no table holds, is shown as hex.
fn value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(integer) => Value::from(integer),
        ValueRef::Real(real) => Value::from(real),
        ValueRef::Text(text) => Value::from(String::from_utf8_lossy(text)),
        ValueRef::Blob(bytes) => Value::from(hex(bytes)),
    }
}

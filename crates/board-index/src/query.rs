//! Reading the index with SQL (docs/architecture.md, The index).

use std::time::{Duration, Instant};

use rusqlite::ErrorCode;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::ValueRef;
use serde_json::Value;

use crate::open::{Index, IndexError, hex};

/// How long one query may run before it is stopped. It bounds how long an
/// agent waits for an answer.
pub(crate) const QUERY_TIME: Duration = Duration::from_secs(1);

/// How many virtual machine steps run between checks of the clock.
const STEPS_PER_CHECK: i32 = 1000;

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
        let started = Instant::now();
        self.connection.progress_handler(
            STEPS_PER_CHECK,
            Some(move || started.elapsed() > QUERY_TIME),
        )?;
        let result = self.read(sql, limit).map_err(blame);
        self.connection
            .progress_handler(STEPS_PER_CHECK, None::<fn() -> bool>)?;
        self.connection.pragma_update(None, "query_only", false)?;
        result
    }

    fn read(&self, sql: &str, limit: usize) -> Result<Rows, rusqlite::Error> {
        // `query_only` does not stop ATTACH, DETACH or a PRAGMA that writes.
        // The authorizer covers only this prepare: a pragma function such as
        // `pragma_table_info` prepares its own PRAGMA while it runs.
        self.connection.authorizer(Some(authorize))?;
        // rusqlite refuses a string holding more than one statement.
        let prepared = self.connection.prepare(sql);
        self.connection
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
        let mut statement = prepared?;
        // Each check covers a gap in the other: `readonly` passes ATTACH, and
        // the authorizer cannot see the file VACUUM INTO attaches as it runs.
        if !statement.readonly() {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_READONLY),
                Some("the statement would write".to_owned()),
            ));
        }
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

/// Refuses ATTACH, DETACH and the PRAGMA statement.
fn authorize(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } | AuthAction::Pragma { .. } => {
            Authorization::Deny
        }
        _ => Authorization::Allow,
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
        Some(ErrorCode::OperationInterrupted) => IndexError::Timeout,
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

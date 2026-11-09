//! Reading the index with SQL (docs/architecture/index.md).

use std::time::{Duration, Instant};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode};
use serde_json::Value;

use crate::open::{IndexError, hex};
use crate::schema::PRUNES;
use crate::store::View;

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
    /// Whether the rows left out start with one that would have taken the
    /// rows past their [`Budget`].
    pub over_budget: bool,
}

/// The bytes a query's rows may take as they are printed: `row` for each
/// row, and `value` for each value in it.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub bytes: usize,
    pub row: usize,
    pub value: fn(&Value) -> usize,
}

impl View {
    /// Runs one statement that cannot write, returning at most `limit` rows,
    /// within `budget` when there is one. It refuses with
    /// [`IndexError::TooBig`] when the first row alone, or any one value, is
    /// past the budget, and with [`IndexError::Pruned`] when another process
    /// has pruned since the views were built.
    pub fn query(
        &self,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
    ) -> Result<Rows, IndexError> {
        // Setting a hook needs the connection alone, so hooks are set here,
        // while no statement or transaction borrows it.
        let mut index = self.index.borrow_mut();
        let connection = &mut index.connection;
        connection.pragma_update(None, "query_only", true)?;
        let started = Instant::now();
        connection.progress_handler(
            STEPS_PER_CHECK,
            Some(move || started.elapsed() > QUERY_TIME),
        )?;
        let before = connection.limit(Limit::SQLITE_LIMIT_LENGTH)?;
        let result = match compile(connection, sql) {
            Ok(()) => self.snapshot(connection, sql, limit, budget),
            Err(error) => Err(self.pruned_or(connection, error)),
        };
        connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, before)?;
        connection.progress_handler(STEPS_PER_CHECK, None::<fn() -> bool>)?;
        connection.pragma_update(None, "query_only", false)?;
        result
    }

    /// [`IndexError::Pruned`] when another process has pruned since the views
    /// were built, so the run builds them again before a statement that did
    /// not compile against them is refused; otherwise `error`.
    fn pruned_or(&self, connection: &Connection, error: IndexError) -> IndexError {
        match connection.query_row(PRUNES, [], |row| row.get::<_, i64>(0)) {
            Ok(prunes) if prunes != self.prunes => IndexError::Pruned,
            _ => error,
        }
    }

    /// Reads the prune count and runs `sql` in one transaction, so no prune
    /// can come between them.
    fn snapshot(
        &self,
        connection: &Connection,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
    ) -> Result<Rows, IndexError> {
        let _snapshot = connection.unchecked_transaction()?;
        let prunes: i64 = connection.query_row(PRUNES, [], |row| row.get(0))?;
        if prunes != self.prunes {
            return Err(IndexError::Pruned);
        }
        Self::read(connection, sql, limit, budget).map_err(blame)
    }

    fn read(
        connection: &Connection,
        sql: &str,
        limit: usize,
        budget: Option<Budget>,
    ) -> Result<Rows, rusqlite::Error> {
        let mut statement = connection.prepare(sql)?;
        // Each check covers a gap in the other: `readonly` passes ATTACH, and
        // the authorizer cannot see the file VACUUM INTO attaches as it runs.
        if !statement.readonly() {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_READONLY),
                Some("the statement would write".to_owned()),
            ));
        }
        let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
        let width = columns.len();
        let mut rows = Vec::new();
        // SQLite raises a length limit under 30 bytes to 30.
        if let Some(Budget { bytes, .. }) = budget {
            let length = i32::try_from(bytes).unwrap_or(i32::MAX);
            connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, length)?;
        }
        let mut cursor = statement.query([])?;
        let mut truncated = false;
        let mut over_budget = false;
        let mut spent = 0;
        while let Some(row) = cursor.next()? {
            if rows.len() == limit {
                truncated = true;
                break;
            }
            let mut values = Vec::with_capacity(width);
            let mut fits = true;
            if let Some(Budget { row, .. }) = budget {
                spent += row;
            }
            for at in 0..width {
                let value = value(row.get_ref(at)?);
                if let Some(budget) = budget {
                    spent += (budget.value)(&value);
                    if spent > budget.bytes {
                        fits = false;
                        break;
                    }
                }
                values.push(value);
            }
            if !fits {
                if rows.is_empty() {
                    return Err(too_big());
                }
                (truncated, over_budget) = (true, true);
                break;
            }
            rows.push(values);
        }
        Ok(Rows {
            columns,
            rows,
            truncated,
            over_budget,
        })
    }
}

/// The error `SQLite` gives a value past its length limit.
fn too_big() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_TOOBIG),
        None,
    )
}

/// Compiles `sql` under the authorizer and discards the result, so a refused
/// statement never reaches the transaction. `query_only` does not stop ATTACH,
/// DETACH or a PRAGMA that writes. The authorizer is gone before the statement
/// that runs is compiled: a pragma function such as `pragma_table_info`
/// prepares its own PRAGMA as it runs, and rusqlite's authorizer panics on an
/// argument that is not UTF-8, which that PRAGMA can hold.
fn compile(connection: &mut Connection, sql: &str) -> Result<(), IndexError> {
    connection.authorizer(Some(authorize))?;
    // rusqlite refuses a string holding more than one statement.
    let compiled = connection.prepare(sql).map(drop).map_err(blame);
    connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
    compiled
}

/// Refuses ATTACH, DETACH and the PRAGMA statement. When the file or name in
/// an ATTACH or DETACH is an expression, `SQLite` passes no argument and
/// rusqlite reports an action it cannot name, so that is refused too.
fn authorize(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Attach { .. }
        | AuthAction::Detach { .. }
        | AuthAction::Pragma { .. }
        | AuthAction::Unknown { .. } => Authorization::Deny,
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
        Some(ErrorCode::TooBig) => IndexError::TooBig,
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

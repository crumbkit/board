//! Reading the index with SQL (docs/architecture.md, The index).

use rusqlite::types::ValueRef;
use serde_json::Value;

use crate::open::{Index, IndexError, hex};

/// What one statement returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Index {
    /// Runs one statement that cannot write.
    pub fn query(&self, sql: &str) -> Result<Rows, IndexError> {
        self.connection.pragma_update(None, "query_only", true)?;
        let result = self.read(sql);
        self.connection.pragma_update(None, "query_only", false)?;
        result
    }

    fn read(&self, sql: &str) -> Result<Rows, IndexError> {
        let mut statement = self.connection.prepare(sql)?;
        let columns: Vec<String> = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let width = columns.len();
        let rows = statement
            .query_map([], |row| {
                (0..width)
                    .map(|at| Ok(value(row.get_ref(at)?)))
                    .collect::<Result<Vec<Value>, rusqlite::Error>>()
            })?
            .collect::<Result<_, _>>()?;
        Ok(Rows { columns, rows })
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

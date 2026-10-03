//! Writing facts. Each is keyed by content alone, so a fact once written is
//! true wherever its key appears and is never updated.

use std::collections::HashMap;

use board_model::{Code, Kind, LogicalPath, ObjectId, RecordType, Role, Stat};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::open::{Index, IndexError};

/// What parsing one blob in one role found. For a schema, the code includes
/// whether it compiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobFact {
    pub kind: Kind,
    pub record_type: Option<RecordType>,
    pub code: Option<Code>,
}

/// What checking one record blob against one schema blob found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationFact {
    /// It validates and makes these references.
    Valid(Vec<RefFact>),
    /// It fails with `code` at the JSON pointer `pointer`.
    Invalid { code: Code, pointer: String },
}

/// One reference a record makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefFact {
    /// Where it is in the record, as a JSON pointer.
    pub pointer: String,
    pub target: LogicalPath,
    /// The record types the target may have; `None` accepts any.
    pub accepts: Option<Vec<RecordType>>,
}

/// One working-tree entry as the last refresh saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeRow {
    pub logical: Option<LogicalPath>,
    /// The first problem on the entry's path, or the code it cannot be read
    /// with.
    pub code: Option<Code>,
    pub blob: Option<ObjectId>,
    pub stat: Option<Stat>,
}

/// One write to the index, holding its write lock until it commits.
#[derive(Debug)]
pub struct Writer<'a> {
    transaction: Transaction<'a>,
}

impl Index {
    pub fn write(&mut self) -> Result<Writer<'_>, IndexError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Ok(Writer { transaction })
    }
}

impl Writer<'_> {
    pub fn blob(&self, blob: ObjectId, role: Role) -> Result<Option<BlobFact>, IndexError> {
        let row: Option<(String, Option<String>, Option<String>)> = self
            .transaction
            .query_row(
                "SELECT kind, type, code FROM blobs WHERE blob = ?1 AND role = ?2",
                params![blob.to_string(), role.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(row.and_then(|(kind, record_type, code)| {
            Some(BlobFact {
                kind: Kind::ALL.into_iter().find(|known| known.as_str() == kind)?,
                record_type: match record_type {
                    Some(text) => Some(text.parse().ok()?),
                    None => None,
                },
                code: match code {
                    Some(text) => Some(Code::parse(&text)?),
                    None => None,
                },
            })
        }))
    }

    pub fn put_blob(&self, blob: ObjectId, role: Role, fact: &BlobFact) -> Result<(), IndexError> {
        self.transaction.execute(
            "INSERT OR REPLACE INTO blobs (blob, role, kind, type, code)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                blob.to_string(),
                role.as_str(),
                fact.kind.as_str(),
                fact.record_type.as_ref().map(RecordType::as_str),
                fact.code.map(Code::as_str),
            ],
        )?;
        Ok(())
    }

    /// What checking `record` against `schema` found, if it has been
    /// checked.
    pub fn validation(
        &self,
        record: ObjectId,
        schema: ObjectId,
    ) -> Result<Option<ValidationFact>, IndexError> {
        let keys = params![record.to_string(), schema.to_string()];
        let row: Option<(Option<String>, Option<String>)> = self
            .transaction
            .query_row(
                "SELECT code, pointer FROM validations WHERE record = ?1 AND schema = ?2",
                keys,
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((code, pointer)) = row else {
            return Ok(None);
        };
        if let Some(code) = code {
            return Ok(Code::parse(&code).map(|code| ValidationFact::Invalid {
                code,
                pointer: pointer.unwrap_or_default(),
            }));
        }
        let mut statement = self.transaction.prepare(
            "SELECT pointer, target, accepts FROM refs
             WHERE record = ?1 AND schema = ?2 ORDER BY ordinal",
        )?;
        let references = statement
            .query_map(keys, |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .map(|row| {
                let (pointer, target, accepts) = row?;
                Ok(target.parse().ok().map(|target| RefFact {
                    pointer,
                    target,
                    accepts: accepts.and_then(|text| serde_json::from_str(&text).ok()),
                }))
            })
            .collect::<Result<Option<Vec<RefFact>>, IndexError>>()?;
        Ok(references.map(ValidationFact::Valid))
    }

    pub fn put_validation(
        &self,
        record: ObjectId,
        schema: ObjectId,
        fact: &ValidationFact,
    ) -> Result<(), IndexError> {
        let (record, schema) = (record.to_string(), schema.to_string());
        let (code, pointer) = match fact {
            ValidationFact::Valid(_) => (None, None),
            ValidationFact::Invalid { code, pointer } => (Some(code.as_str()), Some(pointer)),
        };
        self.transaction.execute(
            "INSERT OR REPLACE INTO validations (record, schema, code, pointer)
             VALUES (?1, ?2, ?3, ?4)",
            params![record, schema, code, pointer],
        )?;
        let ValidationFact::Valid(references) = fact else {
            return Ok(());
        };
        for (ordinal, reference) in (0_i64..).zip(references) {
            let accepts = reference.accepts.as_ref().map(|types| {
                serde_json::Value::from(types.iter().map(RecordType::as_str).collect::<Vec<_>>())
                    .to_string()
            });
            self.transaction.execute(
                "INSERT OR REPLACE INTO refs (record, schema, ordinal, pointer, target, accepts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    record,
                    schema,
                    ordinal,
                    reference.pointer,
                    reference.target.to_string(),
                    accepts
                ],
            )?;
        }
        Ok(())
    }

    /// When the last refresh began, in nanoseconds since the Unix epoch.
    pub fn refreshed_ns(&self) -> Result<Option<i64>, IndexError> {
        let value: Option<String> = self
            .transaction
            .query_row(
                "SELECT value FROM state WHERE key = 'refreshed_ns'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.and_then(|value| value.parse().ok()))
    }

    pub fn set_refreshed_ns(&self, started_ns: i64) -> Result<(), IndexError> {
        self.transaction.execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('refreshed_ns', ?1)",
            [started_ns.to_string()],
        )?;
        Ok(())
    }

    /// Every working-tree entry as the last refresh saw it, by path.
    pub fn worktree(&self) -> Result<HashMap<String, WorktreeRow>, IndexError> {
        let mut statement = self.transaction.prepare(
            "SELECT path, logical, code, blob, size, modified_ns, changed_ns, inode, mode
             FROM worktree",
        )?;
        let rows = statement.query_map([], |row| {
            let stat = match (
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ) {
                (Some(size), Some(modified_ns), Some(changed_ns), Some(inode), Some(mode)) => {
                    Some(Stat {
                        size: i64::cast_unsigned(size),
                        modified_ns,
                        changed_ns,
                        inode: i64::cast_unsigned(inode),
                        mode,
                    })
                }
                _ => None,
            };
            let text = |at| row.get::<_, Option<String>>(at);
            Ok((
                row.get::<_, String>(0)?,
                WorktreeRow {
                    logical: text(1)?.and_then(|text| text.parse().ok()),
                    code: text(2)?.and_then(|text| Code::parse(&text)),
                    blob: text(3)?.and_then(|text| ObjectId::from_hex(&text)),
                    stat,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn put_worktree(&self, path: &str, row: &WorktreeRow) -> Result<(), IndexError> {
        let stat = row.stat.as_ref();
        self.transaction.execute(
            "INSERT OR REPLACE INTO worktree
             (path, logical, code, blob, size, modified_ns, changed_ns, inode, mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                path,
                row.logical.as_ref().map(ToString::to_string),
                row.code.map(Code::as_str),
                row.blob.map(|blob| blob.to_string()),
                stat.map(|stat| stat.size.cast_signed()),
                stat.map(|stat| stat.modified_ns),
                stat.map(|stat| stat.changed_ns),
                stat.map(|stat| stat.inode.cast_signed()),
                stat.map(|stat| stat.mode),
            ],
        )?;
        Ok(())
    }

    pub fn delete_worktree(&self, path: &str) -> Result<(), IndexError> {
        self.transaction
            .execute("DELETE FROM worktree WHERE path = ?1", [path])?;
        Ok(())
    }

    pub fn commit(self) -> Result<(), IndexError> {
        Ok(self.transaction.commit()?)
    }
}

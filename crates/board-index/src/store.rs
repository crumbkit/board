//! Writing facts. Each is keyed by content alone, so a fact once written is
//! true wherever its key appears and is never updated.

use std::collections::HashMap;

use std::path::Path;

use board_model::{
    Change, Code, Kind, LogicalPath, ObjectId, RecordType, Role, Source, Stat, canonical_json,
};
use board_tree::{MountPoint, TreeRow, TreeRows};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::open::{Index, IndexError};
use crate::schema::RESOLVED;

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
    /// It fails with `code` at the JSON pointer `pointer`, for `reason`,
    /// the first of `failures`.
    Invalid {
        code: Code,
        pointer: String,
        reason: String,
        failures: u64,
    },
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

/// git's mode for a submodule, whose object is a commit.
const SUBMODULE_MODE: u32 = 0o160_000;

/// How the index names a source: `worktree`, or a commit's id. A commit in
/// two clones has the same content, so it is one source.
pub fn source_name(source: &Source) -> String {
    match source {
        Source::WorkingTree => "worktree".to_owned(),
        Source::Commit(commit) => commit.to_string(),
    }
}

/// What one run decided about one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub source: Source,
    pub path: String,
    pub logical: Option<LogicalPath>,
    pub blob: Option<ObjectId>,
    pub size: Option<u64>,
    pub role: Option<Role>,
    /// The schema blob a record was checked against.
    pub schema: Option<ObjectId>,
    pub kind: Kind,
    pub code: Option<Code>,
    pub change: Change,
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
            .prepare_cached(
                "SELECT kind, type, code FROM main.blobs WHERE blob = ?1 AND role = ?2",
            )?
            .query_row(params![blob.to_string(), role.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
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
        self.transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO main.blobs (blob, role, kind, type, code)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                blob.to_string(),
                role.as_str(),
                fact.kind.as_str(),
                fact.record_type.as_ref().map(RecordType::as_str),
                fact.code.map(Code::as_str),
            ])?;
        Ok(())
    }

    /// Stores the canonical form of the record `blob` parses to.
    pub fn put_body(&self, blob: ObjectId, value: &serde_json::Value) -> Result<(), IndexError> {
        self.transaction
            .prepare_cached("INSERT OR IGNORE INTO main.bodies (blob, body) VALUES (?1, ?2)")?
            .execute(params![
                blob.to_string(),
                String::from_utf8_lossy(&canonical_json(value)),
            ])?;
        Ok(())
    }

    /// What checking `record` against `schema` found, if it has been
    /// checked.
    pub fn validation(
        &self,
        record: ObjectId,
        schema: ObjectId,
    ) -> Result<Option<ValidationFact>, IndexError> {
        type Row = (Option<String>, Option<String>, Option<String>, Option<i64>);
        let keys = params![record.to_string(), schema.to_string()];
        let row: Option<Row> = self
            .transaction
            .prepare_cached(
                "SELECT code, pointer, reason, failures FROM main.validations
                 WHERE record = ?1 AND schema = ?2",
            )?
            .query_row(keys, |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?;
        let Some((code, pointer, reason, failures)) = row else {
            return Ok(None);
        };
        if let Some(code) = code {
            return Ok(Code::parse(&code).map(|code| ValidationFact::Invalid {
                code,
                pointer: pointer.unwrap_or_default(),
                reason: reason.unwrap_or_default(),
                failures: failures.unwrap_or_default().cast_unsigned(),
            }));
        }
        let mut statement = self.transaction.prepare_cached(
            "SELECT pointer, target, accepts FROM main.refs
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
        let (code, pointer, reason, failures) = match fact {
            ValidationFact::Valid(_) => (None, None, None, None),
            ValidationFact::Invalid {
                code,
                pointer,
                reason,
                failures,
            } => (
                Some(code.as_str()),
                Some(pointer),
                Some(reason),
                Some(failures.cast_signed()),
            ),
        };
        self.transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO main.validations
                 (record, schema, code, pointer, reason, failures)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![record, schema, code, pointer, reason, failures])?;
        let ValidationFact::Valid(references) = fact else {
            return Ok(());
        };
        for (ordinal, reference) in (0_i64..).zip(references) {
            let accepts = reference.accepts.as_ref().map(|types| {
                serde_json::Value::from(types.iter().map(RecordType::as_str).collect::<Vec<_>>())
                    .to_string()
            });
            self.transaction.prepare_cached("INSERT OR REPLACE INTO main.refs (record, schema, ordinal, pointer, target, accepts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?
            .execute(params![
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
            .prepare_cached("SELECT value FROM main.state WHERE key = 'refreshed_ns'")?
            .query_row([], |row| row.get(0))
            .optional()?;
        Ok(value.and_then(|value| value.parse().ok()))
    }

    pub fn set_refreshed_ns(&self, started_ns: i64) -> Result<(), IndexError> {
        self.transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO main.state (key, value) VALUES ('refreshed_ns', ?1)",
            )?
            .execute([started_ns.to_string()])?;
        Ok(())
    }

    /// Every working-tree entry as the last refresh saw it, by path.
    pub fn worktree(&self) -> Result<HashMap<String, WorktreeRow>, IndexError> {
        let mut statement = self.transaction.prepare_cached(
            "SELECT path, logical, code, blob, size, modified_ns, changed_ns, inode, mode
             FROM main.worktree",
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
        self.transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO main.worktree
             (path, logical, code, blob, size, modified_ns, changed_ns, inode, mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?
            .execute(params![
                path,
                row.logical.as_ref().map(ToString::to_string),
                row.code.map(Code::as_str),
                row.blob.map(|blob| blob.to_string()),
                stat.map(|stat| stat.size.cast_signed()),
                stat.map(|stat| stat.modified_ns),
                stat.map(|stat| stat.changed_ns),
                stat.map(|stat| stat.inode.cast_signed()),
                stat.map(|stat| stat.mode),
            ])?;
        Ok(())
    }

    pub fn delete_worktree(&self, path: &str) -> Result<(), IndexError> {
        self.transaction
            .prepare_cached("DELETE FROM main.worktree WHERE path = ?1")?
            .execute([path])?;
        Ok(())
    }

    pub fn has_tree(&self, tree: ObjectId) -> Result<bool, IndexError> {
        Ok(self
            .transaction
            .prepare_cached("SELECT 1 FROM main.trees WHERE tree = ?1 LIMIT 1")?
            .query_row([tree.to_string()], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn put_tree(&self, tree: &TreeRows) -> Result<(), IndexError> {
        let id = tree.id.to_string();
        for row in &tree.rows {
            self.transaction
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.trees (tree, name, mode, object, size, segment, code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?
                .execute(params![
                    id,
                    row.name,
                    row.mode,
                    row.object.to_string(),
                    row.size.map(u64::cast_signed),
                    row.segment,
                    row.code.map(Code::as_str),
                ])?;
        }
        Ok(())
    }

    /// The rows of `tree`, or `None` when the index does not hold it.
    pub fn tree(&self, tree: ObjectId) -> Result<Option<Vec<TreeRow>>, IndexError> {
        let mut statement = self.transaction.prepare_cached(
            "SELECT name, mode, object, size, segment, code FROM main.trees WHERE tree = ?1",
        )?;
        let rows = statement
            .query_map([tree.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .map(|row| {
                let (name, mode, object, size, segment, code) = row?;
                Ok(ObjectId::from_hex(&object).map(|object| TreeRow {
                    name,
                    mode,
                    object,
                    size: size.map(i64::cast_unsigned),
                    segment,
                    code: code.and_then(|code| Code::parse(&code)),
                }))
            })
            .collect::<Result<Option<Vec<TreeRow>>, IndexError>>()?;
        Ok(rows.filter(|rows| !rows.is_empty()))
    }

    /// Replaces the sources and mounts the last refresh found.
    pub fn set_mounts(
        &self,
        sources: &[(Source, Option<ObjectId>, &Path)],
        mounts: &[MountPoint],
    ) -> Result<(), IndexError> {
        self.transaction
            .execute_batch("DELETE FROM main.sources; DELETE FROM main.mounts;")?;
        for (source, tree, repository) in sources {
            self.transaction
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.sources (source, tree, repository) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![
                    source_name(source),
                    tree.map(|tree| tree.to_string()),
                    repository.to_string_lossy(),
                ])?;
        }
        for mount in mounts {
            self.transaction
                .prepare_cached("INSERT OR REPLACE INTO main.mounts (source, path, target, code) VALUES (?1, ?2, ?3, ?4)")?
                .execute(params![
                    source_name(&mount.source),
                    mount.path,
                    mount.target.ok().map(|target| target.to_string()),
                    mount.target.err().map(Code::as_str),
                ])?;
        }
        Ok(())
    }

    /// Every file, symlink and submodule beneath the tree `root`, by
    /// `/`-separated path, with its blob; a submodule has none.
    pub fn files(&self, root: ObjectId) -> Result<HashMap<String, Option<ObjectId>>, IndexError> {
        let mut statement = self.transaction.prepare_cached(
            "WITH RECURSIVE walk (path, mode, object) AS (
                 SELECT name, mode, object FROM main.trees WHERE tree = ?1
                 UNION ALL
                 SELECT walk.path || '/' || trees.name, trees.mode, trees.object
                 FROM walk JOIN main.trees AS trees ON trees.tree = walk.object
                 WHERE walk.mode = 16384
             )
             SELECT path, mode, object FROM walk WHERE mode <> 16384",
        )?;
        let rows = statement.query_map([root.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut files = HashMap::new();
        for row in rows {
            let (path, mode, object) = row?;
            let blob = (mode != SUBMODULE_MODE)
                .then(|| ObjectId::from_hex(&object))
                .flatten();
            files.insert(path, blob);
        }
        Ok(files)
    }

    /// Replaces what this run decided, which the `entries` view shows.
    pub fn resolve(&self, entries: &[Resolved]) -> Result<(), IndexError> {
        self.transaction.execute_batch(RESOLVED)?;
        let mut statement = self.transaction.prepare_cached(
            "INSERT INTO temp.resolved
             (source, path, logical, blob, size, role, schema, kind, code, change)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for entry in entries {
            statement.execute(params![
                source_name(&entry.source),
                entry.path,
                entry.logical.as_ref().map(ToString::to_string),
                entry.blob.map(|blob| blob.to_string()),
                entry.size.map(u64::cast_signed),
                entry.role.map(Role::as_str),
                entry.schema.map(|schema| schema.to_string()),
                entry.kind.as_str(),
                entry.code.map(Code::as_str),
                entry.change.as_str(),
            ])?;
        }
        Ok(())
    }

    pub fn commit(self) -> Result<(), IndexError> {
        Ok(self.transaction.commit()?)
    }
}

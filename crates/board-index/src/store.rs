//! Writing facts. Each is keyed by content alone, so a fact once written is
//! true wherever its key appears and is never updated.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use board_model::{Canonical, Change, Code, Kind, LogicalPath, ObjectId, RecordType, Role, Source};
use board_tree::{MountPoint, TreeRow, TreeRows};
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, TransactionState, params};

use crate::open::{Index, IndexError, Storage};
use crate::schema::{COUNTED, KEPT, PRUNE, PRUNES, REACHED};

/// What parsing one blob in one role found. For a schema, the code includes
/// whether it compiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobFact {
    pub kind: Kind,
    pub record_type: Option<RecordType>,
    pub code: Option<Code>,
    /// Why the blob fails with `code`, when the parser or schema compiler
    /// says more than the code.
    pub reason: Option<String>,
    /// For a Markdown file, the destination of each link that names a board
    /// path, as written and in order.
    pub links: Vec<String>,
}

/// A `blobs` row as stored: kind, type, code and reason.
pub(crate) type BlobRow = (String, Option<String>, Option<String>, Option<String>);

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
    /// Whether the target must be in the record's own board.
    pub local: bool,
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

/// One transaction on the index that may write. It reads one snapshot, and
/// holds the write lock from its first write until it decides. It owns the
/// index, so dropping it closes the connection, which rolls back.
#[derive(Debug)]
pub struct Writer {
    index: Index,
    /// How many validations it has looked up.
    validations_read: Cell<usize>,
    /// How many blob facts it has looked up.
    blobs_read: Cell<usize>,
}

/// The index once a run has committed what it decided. It holds the entries
/// the run decided, and only [`Decided::view`] fills the views with them, so
/// a run that reports from its own entries, as `check` and `ls` do, never
/// pays for the fill.
#[derive(Debug)]
pub struct Decided {
    pub(crate) index: Index,
    /// The prune count the run decided under.
    pub(crate) prunes: i64,
    /// The entries the run decided and the index does not keep, which
    /// [`Decided::view`] fills the views with.
    pub(crate) entries: Vec<Resolved>,
}

/// The index once its views are filled with what a run decided. Only a view
/// reads them.
#[derive(Debug)]
pub struct View {
    /// In a cell because a query sets hooks on the connection, which rusqlite
    /// allows only through an exclusive reference, and [`View::query`] takes
    /// a shared one.
    pub(crate) index: RefCell<Index>,
    storage: Storage,
    /// The prune count the views were built under.
    pub(crate) prunes: i64,
}

/// A row of `sources`, and of `mounts`, as stored.
pub(crate) type SourceRow = (String, Option<String>, String);
pub(crate) type MountRow = (String, String, Option<String>, Option<String>);

impl Index {
    /// A transaction that takes the write lock now, waiting for it.
    pub fn write(self) -> Result<Writer, IndexError> {
        self.begin("BEGIN IMMEDIATE")
    }

    /// A transaction that takes the write lock at its first write, as
    /// `BEGIN DEFERRED` does. That write fails at once with
    /// [`IndexError::Busy`] if another process holds the lock or has written
    /// since this transaction began reading.
    pub fn deferred(self) -> Result<Writer, IndexError> {
        self.begin("BEGIN DEFERRED")
    }

    fn begin(self, sql: &str) -> Result<Writer, IndexError> {
        self.connection.execute_batch(sql)?;
        self.connection.execute_batch(REACHED)?;
        Ok(Writer {
            index: self,
            validations_read: Cell::new(0),
            blobs_read: Cell::new(0),
        })
    }
}

impl View {
    pub(crate) fn new(index: Index, prunes: i64) -> Self {
        Self {
            storage: index.storage().clone(),
            index: RefCell::new(index),
            prunes,
        }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> std::cell::Ref<'_, Connection> {
        std::cell::Ref::map(self.index.borrow(), |index| &index.connection)
    }
}

impl Writer {
    pub(crate) fn connection(&self) -> &Connection {
        &self.index.connection
    }

    pub fn blob(&self, blob: ObjectId, role: Role) -> Result<Option<BlobFact>, IndexError> {
        self.blobs_read.set(self.blobs_read.get() + 1);
        let row: Option<BlobRow> = self
            .connection()
            .prepare_cached(
                "SELECT kind, type, code, reason FROM main.blobs WHERE blob = ?1 AND role = ?2",
            )?
            .query_row(params![blob.to_string(), role.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?;
        let links = match (&row, role) {
            (Some(_), Role::Markdown) => self
                .connection()
                .prepare_cached(
                    "SELECT destination FROM main.links WHERE blob = ?1 ORDER BY ordinal",
                )?
                .query_map(params![blob.to_string()], |row| row.get(0))?
                .collect::<Result<_, _>>()?,
            _ => Vec::new(),
        };
        Ok(row.and_then(|row| fact(row, links)))
    }

    pub fn put_blob(&self, blob: ObjectId, role: Role, fact: &BlobFact) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached(
                "INSERT OR REPLACE INTO main.blobs (blob, role, kind, type, code, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                blob.to_string(),
                role.as_str(),
                fact.kind.as_str(),
                fact.record_type.as_ref().map(RecordType::as_str),
                fact.code.map(Code::as_str),
                fact.reason,
            ])?;
        for (ordinal, link) in (0_i64..).zip(&fact.links) {
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.links (blob, ordinal, destination)
                     VALUES (?1, ?2, ?3)",
                )?
                .execute(params![blob.to_string(), ordinal, link])?;
        }
        Ok(())
    }

    /// Stores `body`, the canonical form of the record `blob` parses to.
    pub fn put_body(&self, blob: ObjectId, body: &Canonical) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached("INSERT OR IGNORE INTO main.bodies (blob, body) VALUES (?1, ?2)")?
            .execute(params![blob.to_string(), body.as_str()])?;
        Ok(())
    }

    /// What checking `record` against `schema` found, if it has been
    /// checked.
    pub fn validation(
        &self,
        record: ObjectId,
        schema: ObjectId,
    ) -> Result<Option<ValidationFact>, IndexError> {
        type Outcome = (Option<String>, Option<String>, Option<String>, Option<i64>);
        type Row = (Outcome, Option<(String, String, Option<String>, bool)>);
        self.validations_read.set(self.validations_read.get() + 1);
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT v.code, v.pointer, v.reason, v.failures, r.pointer, r.target, r.accepts,
                        r.local
                 FROM main.validations AS v LEFT JOIN main.refs AS r
                 ON r.record = v.record AND r.schema = v.schema
                 WHERE v.record = ?1 AND v.schema = ?2 ORDER BY r.ordinal",
            )?
            .query_map(params![record.to_string(), schema.to_string()], |row| {
                let reference = match row.get::<_, Option<String>>(4)? {
                    Some(pointer) => Some((pointer, row.get(5)?, row.get(6)?, row.get(7)?)),
                    None => None,
                };
                Ok((
                    (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?),
                    reference,
                ))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        let Some(((code, pointer, reason, failures), _)) = rows.first() else {
            return Ok(None);
        };
        if let Some(code) = code {
            return Ok(Code::parse(code).map(|code| ValidationFact::Invalid {
                code,
                pointer: pointer.clone().unwrap_or_default(),
                reason: reason.clone().unwrap_or_default(),
                failures: failures.unwrap_or_default().cast_unsigned(),
            }));
        }
        let references = rows
            .into_iter()
            .filter_map(|(_, reference)| reference)
            .map(|(pointer, target, accepts, local)| {
                target.parse().ok().map(|target| RefFact {
                    pointer,
                    target,
                    accepts: accepts.and_then(|text| serde_json::from_str(&text).ok()),
                    local,
                })
            })
            .collect::<Option<Vec<RefFact>>>();
        Ok(references.map(ValidationFact::Valid))
    }

    /// How many validations this transaction has looked up. Work-count
    /// tests assert it.
    pub fn validations_read(&self) -> usize {
        self.validations_read.get()
    }

    /// How many blob facts this transaction has looked up. Work-count tests
    /// assert it.
    pub fn blobs_read(&self) -> usize {
        self.blobs_read.get()
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
        self.connection()
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
            self.connection().prepare_cached("INSERT OR REPLACE INTO main.refs (record, schema, ordinal, pointer, target, accepts, local)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?
            .execute(params![
                    record,
                    schema,
                    ordinal,
                    reference.pointer,
                    reference.target.to_string(),
                    accepts,
                    reference.local
                ],
            )?;
        }
        Ok(())
    }

    pub fn has_tree(&self, tree: ObjectId) -> Result<bool, IndexError> {
        Ok(self
            .connection()
            .prepare_cached("SELECT 1 FROM main.trees WHERE tree = ?1 LIMIT 1")?
            .query_row([tree.to_string()], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn put_tree(&self, tree: &TreeRows) -> Result<(), IndexError> {
        let id = tree.id.to_string();
        for row in &tree.rows {
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.trees (tree, name, mode, object, size, segment, code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?
                .execute(params![
                    id,
                    name(&row.name),
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
        let mut statement = self.connection().prepare_cached(
            "SELECT name, mode, object, size, segment, code FROM main.trees WHERE tree = ?1",
        )?;
        let rows = statement
            .query_map([tree.to_string()], |row| {
                Ok((
                    row.get_ref(0)?.as_bytes()?.to_vec(),
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

    /// Replaces the sources and mounts the last refresh found, unless the
    /// index already holds exactly these.
    pub fn set_mounts(
        &self,
        sources: &[(Source, Option<ObjectId>, &Path)],
        mounts: &[MountPoint],
    ) -> Result<(), IndexError> {
        let sources: BTreeSet<SourceRow> = sources
            .iter()
            .map(|(source, tree, repository)| {
                let tree = tree.map(|tree| tree.to_string());
                (
                    source_name(source),
                    tree,
                    repository.to_string_lossy().into(),
                )
            })
            .collect();
        let mounts: BTreeSet<MountRow> = mounts
            .iter()
            .map(|mount| {
                let target = mount.target.ok().map(|target| target.to_string());
                let code = mount.target.err().map(|code| code.as_str().to_owned());
                (source_name(&mount.source), mount.path.clone(), target, code)
            })
            .collect();
        if self.mounted()? == (sources.clone(), mounts.clone()) {
            return Ok(());
        }
        self.connection()
            .execute_batch("DELETE FROM main.sources; DELETE FROM main.mounts;")?;
        for (source, tree, repository) in sources {
            self.connection()
                .prepare_cached(
                    "INSERT INTO main.sources (source, tree, repository) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![source, tree, repository])?;
        }
        for (source, path, target, code) in mounts {
            self.connection()
                .prepare_cached(
                    "INSERT INTO main.mounts (source, path, target, code) VALUES (?1, ?2, ?3, ?4)",
                )?
                .execute(params![source, path, target, code])?;
        }
        Ok(())
    }

    /// The sources and mounts the index holds.
    pub(crate) fn mounted(&self) -> Result<(BTreeSet<SourceRow>, BTreeSet<MountRow>), IndexError> {
        let sources = self
            .connection()
            .prepare_cached("SELECT source, tree, repository FROM main.sources")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let mounts = self
            .connection()
            .prepare_cached("SELECT source, path, target, code FROM main.mounts")?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        Ok((sources, mounts))
    }

    /// Prunes when the index has at least doubled in pages since the last
    /// prune, so pruning costs work in proportion to growth. A new index
    /// records its size instead, so it does not prune what it just built.
    /// Returns the rows it deleted.
    pub fn prune_if_doubled(&self) -> Result<usize, IndexError> {
        let recorded: Option<String> = self
            .connection()
            .prepare_cached("SELECT value FROM main.state WHERE key = 'pruned_pages'")?
            .query_row([], |row| row.get(0))
            .optional()?;
        let pages = self.pages()?;
        match recorded.and_then(|value| value.parse::<i64>().ok()) {
            Some(recorded) if pages < 2 * recorded => Ok(0),
            Some(_) => self.prune(),
            None => {
                self.record(pages)?;
                Ok(0)
            }
        }
    }

    /// Deletes every fact that no source's tree and no working-tree file
    /// reaches, gives the freed pages back to the file, and records its size.
    /// Returns the rows it deleted.
    pub fn prune(&self) -> Result<usize, IndexError> {
        self.connection().execute_batch(KEPT)?;
        let mut deleted = 0;
        for statement in PRUNE {
            deleted += self.connection().execute(statement, [])?;
        }
        self.connection().execute(COUNTED, [])?;
        // Each step frees one page, so it runs to the end.
        let mut vacuum = self
            .connection()
            .prepare("PRAGMA main.incremental_vacuum")?;
        let mut freed = vacuum.query([])?;
        while freed.next()?.is_some() {}
        self.record(self.pages()?)?;
        Ok(deleted)
    }

    /// Records `pages` as the index's size after the last prune.
    fn record(&self, pages: i64) -> Result<(), IndexError> {
        self.connection().execute(
            "INSERT OR REPLACE INTO main.state (key, value) VALUES ('pruned_pages', ?1)",
            [pages.to_string()],
        )?;
        Ok(())
    }

    /// The index's size in pages.
    fn pages(&self) -> Result<i64, IndexError> {
        Ok(self
            .connection()
            .query_row("PRAGMA main.page_count", [], |row| row.get(0))?)
    }

    /// Whether this transaction has written, and so holds the write lock.
    pub fn wrote(&self) -> Result<bool, IndexError> {
        Ok(self.connection().transaction_state(Some("main"))? == TransactionState::Write)
    }

    /// Every file, symlink and submodule beneath the tree `root`, by
    /// `/`-separated path, with its blob; a submodule has none.
    pub fn files(&self, root: ObjectId) -> Result<HashMap<String, Option<ObjectId>>, IndexError> {
        let mut statement = self.connection().prepare_cached(
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
                String::from_utf8_lossy(row.get_ref(0)?.as_bytes()?).into_owned(),
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

    /// Commits what this run put, and returns the index decided, holding
    /// `entries`, what the run decided, which its views can show.
    pub fn decide(self, entries: Vec<Resolved>) -> Result<Decided, IndexError> {
        let prunes = self.connection().query_row(PRUNES, [], |row| row.get(0))?;
        self.connection().execute_batch("COMMIT")?;
        Ok(Decided {
            index: self.index,
            prunes,
            entries,
        })
    }

    /// Rolls back, and returns the index to begin again.
    pub fn rollback(self) -> Result<Index, IndexError> {
        // A failed statement may already have rolled back the transaction.
        if !self.connection().is_autocommit() {
            self.connection().execute_batch("ROLLBACK")?;
        }
        Ok(self.index)
    }
}

/// The fact a `blobs` row and its `links` hold, or `None` when the row does
/// not parse.
pub(crate) fn fact(
    (kind, record_type, code, reason): BlobRow,
    links: Vec<String>,
) -> Option<BlobFact> {
    Some(BlobFact {
        kind: Kind::parse(&kind)?,
        record_type: match record_type {
            Some(text) => Some(text.parse().ok()?),
            None => None,
        },
        code: match code {
            Some(text) => Some(Code::parse(&text)?),
            None => None,
        },
        reason,
        links,
    })
}

/// A tree entry's name as the trees table holds it: text when it is UTF-8,
/// else its bytes as a blob, so that names that show alike stay apart.
fn name(name: &[u8]) -> ToSqlOutput<'_> {
    let text = std::str::from_utf8(name).map(|_| ValueRef::Text(name));
    ToSqlOutput::Borrowed(text.unwrap_or(ValueRef::Blob(name)))
}

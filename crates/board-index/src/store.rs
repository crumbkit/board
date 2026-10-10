//! Writing facts. Each is keyed by content alone, so a fact once written is
//! true wherever its key appears and is never updated.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use board_model::{
    Change, Code, Kind, LogicalPath, ObjectId, RecordType, Role, Source, Stat, canonical_json,
};
use board_tree::{Entry, MountPoint, Status, TreeRow, TreeRows};
use rusqlite::{Connection, OptionalExtension, TransactionState, params};

use crate::open::{Index, IndexError, Storage};
use crate::schema::{COUNTED, KEPT, PRUNE, PRUNES, RESOLVED};

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
}

/// A `blobs` row as stored: kind, type, code and reason.
type BlobRow = (String, Option<String>, Option<String>, Option<String>);

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
#[derive(Debug, PartialEq, Eq)]
struct WorktreeRow {
    logical: Option<LogicalPath>,
    /// The first problem on the entry's path, or the code it cannot be read
    /// with.
    code: Option<Code>,
    blob: Option<ObjectId>,
    stat: Option<Stat>,
}

/// The working tree as the last refresh saw it, for a walk to ask which
/// files it need not hash again.
#[derive(Debug)]
pub struct Worktree {
    rows: HashMap<String, WorktreeRow>,
    refreshed_ns: Option<i64>,
    started_ns: i64,
    hashed: usize,
}

impl Worktree {
    /// The blob stored for `path`, if the file can be trusted to hold it
    /// still; otherwise `None`, and the walk hashes the file. A file that
    /// could not be read and has not changed since is tried again, as a cold
    /// rebuild would, but is not counted as hashed.
    pub fn known(&mut self, path: &str, stat: &Stat) -> Option<ObjectId> {
        let row =
            (self.rows.get(path)).filter(|row| trusted(row.stat.as_ref(), stat, self.refreshed_ns));
        self.hashed += usize::from(row.is_none());
        row.and_then(|row| row.blob)
    }
}

/// Whether a stored blob still holds: the stat is unchanged and, as in git's racy-clean
/// rule, the file was modified in an earlier whole second than the last refresh began.
pub(crate) fn trusted(stored: Option<&Stat>, seen: &Stat, refreshed_ns: Option<i64>) -> bool {
    const SECOND: i64 = 1_000_000_000;
    stored == Some(seen)
        && refreshed_ns
            .is_some_and(|ns| seen.modified_ns.div_euclid(SECOND) < ns.div_euclid(SECOND))
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
    prunes: i64,
    /// The entries the run decided, which [`Decided::view`] fills the views
    /// with.
    entries: Vec<Resolved>,
}

/// The index once its views are filled with what a run decided. Only a view
/// reads them.
#[derive(Debug)]
pub struct View {
    pub(crate) index: Index,
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
        Ok(Writer {
            index: self,
            validations_read: Cell::new(0),
            blobs_read: Cell::new(0),
        })
    }
}

impl Decided {
    pub fn storage(&self) -> &Storage {
        self.index.storage()
    }

    /// Fills the views with the entries this run decided, and returns the
    /// index as a view of them.
    pub fn view(self) -> Result<View, IndexError> {
        let connection = &self.index.connection;
        // One transaction, so the inserts commit together.
        let fill = connection.unchecked_transaction()?;
        fill.execute_batch(RESOLVED)?;
        let mut statement = fill.prepare_cached(
            "INSERT INTO temp.resolved
             (source, path, logical, blob, size, role, schema, kind, code, change)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for entry in &self.entries {
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
        drop(statement);
        fill.commit()?;
        Ok(View {
            index: self.index,
            prunes: self.prunes,
        })
    }
}

impl View {
    pub fn storage(&self) -> &Storage {
        self.index.storage()
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.index.connection
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
        Ok(row.and_then(|(kind, record_type, code, reason)| {
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
                reason,
            })
        }))
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
        Ok(())
    }

    /// Stores the canonical form of the record `blob` parses to.
    pub fn put_body(&self, blob: ObjectId, value: &serde_json::Value) -> Result<(), IndexError> {
        self.connection()
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
        self.validations_read.set(self.validations_read.get() + 1);
        let keys = params![record.to_string(), schema.to_string()];
        let row: Option<Row> = self
            .connection()
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
        let mut statement = self.connection().prepare_cached(
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
            self.connection().prepare_cached("INSERT OR REPLACE INTO main.refs (record, schema, ordinal, pointer, target, accepts)
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

    /// The working tree as the last refresh saw it. The refresh that walks
    /// it begins now.
    pub fn worktree(&self) -> Result<Worktree, IndexError> {
        let refreshed_ns: Option<String> = self
            .connection()
            .prepare_cached("SELECT value FROM main.state WHERE key = 'refreshed_ns'")?
            .query_row([], |row| row.get(0))
            .optional()?;
        let mut statement = self.connection().prepare_cached(
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
        Ok(Worktree {
            rows: rows.collect::<Result<_, _>>()?,
            refreshed_ns: refreshed_ns.and_then(|value| value.parse().ok()),
            started_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
                }),
            hashed: 0,
        })
    }

    /// Stores the `entries` a walk of `worktree` found in place of what it
    /// held, and returns how many files the walk hashed. When it hashed or
    /// this records anything, files must predate this refresh's start to be
    /// trusted; keeping the older start trusts no file it did not.
    pub fn sync_worktree(
        &self,
        worktree: Worktree,
        entries: &[Entry],
    ) -> Result<usize, IndexError> {
        let mut rows = worktree.rows;
        let mut changed = worktree.hashed > 0;
        for entry in entries {
            let row = WorktreeRow {
                logical: entry.logical.clone(),
                code: match entry.status {
                    Status::Ok => None,
                    Status::Degraded(code) => Some(code),
                },
                blob: entry.blob,
                stat: entry.stat,
            };
            if rows.remove(&entry.path).as_ref() == Some(&row) {
                continue;
            }
            changed = true;
            let stat = row.stat.as_ref();
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.worktree
                 (path, logical, code, blob, size, modified_ns, changed_ns, inode, mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )?
                .execute(params![
                    entry.path,
                    row.logical.as_ref().map(ToString::to_string),
                    row.code.map(Code::as_str),
                    row.blob.map(|blob| blob.to_string()),
                    stat.map(|stat| stat.size.cast_signed()),
                    stat.map(|stat| stat.modified_ns),
                    stat.map(|stat| stat.changed_ns),
                    stat.map(|stat| stat.inode.cast_signed()),
                    stat.map(|stat| stat.mode),
                ])?;
        }
        for path in rows.keys() {
            self.connection()
                .prepare_cached("DELETE FROM main.worktree WHERE path = ?1")?
                .execute([path])?;
            changed = true;
        }
        if changed {
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.state (key, value) VALUES ('refreshed_ns', ?1)",
                )?
                .execute([worktree.started_ns.to_string()])?;
        }
        Ok(worktree.hashed)
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
        let mut statement = self.connection().prepare_cached(
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

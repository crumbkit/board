//! The working tree as the last refresh saw it, with what the index holds
//! about each of its blobs, so a warm walk hashes and reads only what
//! changed (docs/architecture/performance/warm/index.md).

use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::time::{SystemTime, UNIX_EPOCH};

use board_model::{Code, LogicalPath, ObjectId, Role, Stat};
use board_tree::{Entry, Status};
use rusqlite::{OptionalExtension, params};

use crate::open::IndexError;
use crate::store::{BlobFact, Writer, fact};

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
    /// The fact of each stored row's blob in the role its path gives it,
    /// until [`Worktree::take_facts`] takes them.
    facts: HashMap<(ObjectId, Role), BlobFact>,
    refreshed_ns: Option<i64>,
    started_ns: i64,
    hashed: usize,
}

impl Worktree {
    /// The fact of each stored row's blob in the role its path gives it,
    /// as [`Writer::blob`] gives it, loaded with the rows. A blob the index
    /// holds no fact for in that role has none.
    pub fn take_facts(&mut self) -> HashMap<(ObjectId, Role), BlobFact> {
        std::mem::take(&mut self.facts)
    }

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

impl Writer {
    /// The working tree as the last refresh saw it, with the fact of each
    /// row's blob in the role its path gives it. One statement reads the
    /// rows with their blobs' facts, and one the links, so the load looks
    /// up no blob on its own. The refresh that walks it
    /// begins now.
    pub fn worktree(&self) -> Result<Worktree, IndexError> {
        let refreshed_ns: Option<String> = self
            .connection()
            .prepare_cached("SELECT value FROM main.state WHERE key = 'refreshed_ns'")?
            .query_row([], |row| row.get(0))
            .optional()?;
        // A blob holds a fact for each role it was read in, so a row comes
        // once for each; only the role its path gives it is kept.
        let mut statement = self.connection().prepare_cached(
            "SELECT w.path, w.logical, w.code, w.blob, w.size, w.modified_ns, w.changed_ns,
                    w.inode, w.mode, b.role, b.kind, b.type, b.code, b.reason
             FROM main.worktree AS w LEFT JOIN main.blobs AS b ON b.blob = w.blob",
        )?;
        let mut rows: HashMap<String, WorktreeRow> = HashMap::new();
        let mut facts = HashMap::new();
        let mut query = statement.query([])?;
        while let Some(row) = query.next()? {
            let path: String = row.get(0)?;
            let role = Role::of(&path);
            let stored = (row.get_ref(9)?.as_str_or_null()).map_err(rusqlite::Error::from)?;
            let role = role.filter(|role| stored == Some(role.as_str()));
            let blob = match rows.entry(path) {
                MapEntry::Occupied(occupied) => occupied.get().blob,
                MapEntry::Vacant(vacant) => vacant.insert(worktree_row(row)?).blob,
            };
            if let (Some(role), Some(blob)) = (role, blob)
                && let MapEntry::Vacant(vacant) = facts.entry((blob, role))
                && let Some(fact) = fact(
                    (row.get(10)?, row.get(11)?, row.get(12)?, row.get(13)?),
                    Vec::new(),
                )
            {
                vacant.insert(fact);
            }
        }
        drop(query);
        self.links_into(&mut facts)?;
        Ok(Worktree {
            rows,
            facts,
            refreshed_ns: refreshed_ns.and_then(|value| value.parse().ok()),
            started_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
                }),
            hashed: 0,
        })
    }

    /// Gives each Markdown fact in `facts` the links its blob holds, in
    /// order. One statement reads the whole table in blob and ordinal
    /// order, which costs less than finding the working tree's blobs in it.
    fn links_into(
        &self,
        facts: &mut HashMap<(ObjectId, Role), BlobFact>,
    ) -> Result<(), IndexError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT blob, destination FROM main.links ORDER BY blob, ordinal")?;
        let mut query = statement.query([])?;
        let mut current: Option<(String, Option<ObjectId>)> = None;
        while let Some(row) = query.next()? {
            let text = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
            let blob = match &current {
                Some((last, blob)) if last == text => *blob,
                _ => {
                    let blob = ObjectId::from_hex(text);
                    current = Some((text.to_owned(), blob));
                    blob
                }
            };
            if let Some(fact) = blob.and_then(|blob| facts.get_mut(&(blob, Role::Markdown))) {
                fact.links.push(row.get(1)?);
            }
        }
        Ok(())
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
}

/// A `worktree` row's own columns, the first nine of `row`.
fn worktree_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorktreeRow> {
    let stat = match (
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ) {
        (Some(size), Some(modified_ns), Some(changed_ns), Some(inode), Some(mode)) => Some(Stat {
            size: i64::cast_unsigned(size),
            modified_ns,
            changed_ns,
            inode: i64::cast_unsigned(inode),
            mode,
        }),
        _ => None,
    };
    let text = |at| row.get::<_, Option<String>>(at);
    Ok(WorktreeRow {
        logical: text(1)?.and_then(|text| text.parse().ok()),
        code: text(2)?.and_then(|text| Code::parse(&text)),
        blob: text(3)?.and_then(|text| ObjectId::from_hex(&text)),
        stat,
    })
}

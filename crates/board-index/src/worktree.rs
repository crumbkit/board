//! The working tree as the last refresh saw it, with what the index holds
//! about each of its blobs, so a warm walk hashes and reads only what
//! changed (docs/architecture/performance/warm/index.md).

use std::cmp::Ordering;
use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use board_model::{Code, LogicalPath, ObjectId, Role, Stat};
use board_tree::{Entry, Status};
use rusqlite::{OptionalExtension, params};

use crate::open::IndexError;
use crate::store::{BlobFact, Writer, fact};

/// One working-tree entry as the last refresh saw it.
#[derive(Debug, PartialEq, Eq)]
struct WorktreeRow {
    /// Where its path is in [`Worktree::paths`].
    path: Range<usize>,
    logical: Logical,
    /// The first problem on the entry's path, or the code it cannot be read
    /// with.
    code: Option<Code>,
    blob: Option<ObjectId>,
    stat: Option<Stat>,
}

/// A row's logical path as stored.
#[derive(Debug, PartialEq, Eq)]
enum Logical {
    /// The one a file has when nothing addresses it otherwise, which its
    /// path gives, so the load copies no text for it.
    Own,
    Stored(Option<String>),
}

/// Whether `logical` is the logical path `path` gives a file when nothing
/// addresses it otherwise: the path after a `/`, without `.json`.
fn own(path: &str, logical: &str) -> bool {
    logical.strip_prefix('/') == Some(path.strip_suffix(".json").unwrap_or(path))
}

/// The working tree as the last refresh saw it, for a walk to ask which
/// files it need not hash again.
#[derive(Debug)]
pub struct Worktree {
    /// Each stored row's path, one after another, so the load makes one
    /// allocation for them.
    paths: String,
    /// Each stored row, in path order.
    rows: Vec<WorktreeRow>,
    /// Where the next path a walk asks about is likeliest to be: after the
    /// last one found, since a walk asks in nearly path order.
    next: usize,
    /// The fact of each stored row's blob in the role its path gives it,
    /// until [`Worktree::take_facts`] takes them.
    facts: StoredFacts,
    refreshed_ns: Option<i64>,
    started_ns: i64,
    hashed: usize,
}

/// The fact the index holds for each blob of the working tree's stored
/// rows in the role its path gives it, in the order the index keeps blobs.
#[derive(Default)]
pub struct StoredFacts {
    facts: Vec<((ObjectId, Role), BlobFact)>,
    /// For each value of a blob's first two bytes, where the facts whose
    /// blobs begin with it start, then where the last ends. Blobs are
    /// hashes, so each such run holds about one fact, and finding one reads
    /// no other.
    starts: Vec<usize>,
}

/// A blob's first two bytes, as a number.
fn bucket(blob: &ObjectId) -> usize {
    (blob.as_bytes().first_chunk::<2>()).map_or(0, |lead| usize::from(u16::from_be_bytes(*lead)))
}

impl StoredFacts {
    /// `facts`, in the index's order.
    fn new(facts: Vec<((ObjectId, Role), BlobFact)>) -> Self {
        let mut starts = Vec::with_capacity(usize::from(u16::MAX) + 2);
        let mut at = 0;
        for value in 0..=usize::from(u16::MAX) + 1 {
            while facts
                .get(at)
                .is_some_and(|((blob, _), _)| bucket(blob) < value)
            {
                at += 1;
            }
            starts.push(at);
        }
        Self { facts, starts }
    }

    /// The fact held for `blob` in `role`, if the index holds one.
    pub fn get(&self, blob: ObjectId, role: Role) -> Option<&BlobFact> {
        Some(&self.facts[self.find(blob, role)?].1)
    }

    /// Where the fact for `blob` in `role` is.
    fn find(&self, blob: ObjectId, role: Role) -> Option<usize> {
        let at = bucket(&blob);
        let (&start, &end) = (self.starts.get(at)?, self.starts.get(at + 1)?);
        let found = self.facts[start..end]
            .binary_search_by(|((held, as_role), _)| order((held, *as_role), (&blob, role)))
            .ok()?;
        Some(start + found)
    }

    /// Each blob and role with its fact, in blob order.
    pub fn iter(&self) -> impl Iterator<Item = (&(ObjectId, Role), &BlobFact)> {
        self.facts.iter().map(|(key, fact)| (key, fact))
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

impl std::fmt::Debug for StoredFacts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

/// How the index orders a blob and role: by the blob's bytes, as its hex
/// orders, then by the role's spelling. The first eight bytes decide
/// nearly every comparison, so they are compared as one number.
fn order((blob, role): (&ObjectId, Role), (other, other_role): (&ObjectId, Role)) -> Ordering {
    let lead = |blob: &ObjectId| {
        (blob.as_bytes().first_chunk::<8>()).map_or(0, |lead| u64::from_be_bytes(*lead))
    };
    (lead(blob).cmp(&lead(other)))
        .then_with(|| blob.as_bytes().cmp(other.as_bytes()))
        .then_with(|| rank(role).cmp(&rank(other_role)))
}

/// Where `role` sorts among the roles by its spelling.
const fn rank(role: Role) -> u8 {
    match role {
        Role::Json => 0,
        Role::Markdown => 1,
        Role::ReservedSchema => 2,
        Role::Schema => 3,
    }
}

impl WorktreeRow {
    /// Whether this row holds what a walk found as `entry`, which has the
    /// row's path.
    fn holds(&self, entry: &Entry) -> bool {
        let code = match entry.status {
            Status::Ok => None,
            Status::Degraded(code) => Some(code),
        };
        let logical = entry.logical.as_ref().map(LogicalPath::as_str);
        let logical = match &self.logical {
            Logical::Own => logical.is_some_and(|logical| own(&entry.path, logical)),
            Logical::Stored(stored) => stored.as_deref() == logical,
        };
        logical && self.code == code && self.blob == entry.blob && self.stat == entry.stat
    }
}

impl Worktree {
    /// The fact of each stored row's blob in the role its path gives it,
    /// as [`Writer::blob`] gives it, loaded with the rows. A blob the index
    /// holds no fact for in that role has none.
    pub fn take_facts(&mut self) -> StoredFacts {
        std::mem::take(&mut self.facts)
    }

    /// The blob stored for `path`, if the file can be trusted to hold it
    /// still; otherwise `None`, and the walk hashes the file. A file that
    /// could not be read and has not changed since is tried again, as a cold
    /// rebuild would, but is not counted as hashed.
    pub fn known(&mut self, path: &str, stat: &Stat) -> Option<ObjectId> {
        let row = (self.find(path).map(|at| &self.rows[at]))
            .filter(|row| trusted(row.stat.as_ref(), stat, self.refreshed_ns));
        self.hashed += usize::from(row.is_none());
        row.and_then(|row| row.blob)
    }

    /// Where the row for `path` is, looked for first where the last one
    /// found left off.
    fn find(&mut self, path: &str) -> Option<usize> {
        let paths = &self.paths;
        let at = match self.rows.get(self.next) {
            Some(next) if paths[next.path.clone()] == *path => self.next,
            _ => (self.rows)
                .binary_search_by(|row| paths[row.path.clone()].cmp(path))
                .ok()?,
        };
        self.next = at + 1;
        Some(at)
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
    /// row's blob in the role its path gives it. One ordered scan reads the
    /// rows, one the blobs table and one the links, each matched to the
    /// rows' blobs in blob order here, so the load seeks no blob and hashes
    /// none. The refresh that walks it begins now.
    pub fn worktree(&self) -> Result<Worktree, IndexError> {
        let refreshed_ns: Option<String> = self
            .connection()
            .prepare_cached("SELECT value FROM main.state WHERE key = 'refreshed_ns'")?
            .query_row([], |row| row.get(0))
            .optional()?;
        let mut statement = self.connection().prepare_cached(
            "SELECT path, logical, code, blob, size, modified_ns, changed_ns, inode, mode
             FROM main.worktree ORDER BY path",
        )?;
        let (mut paths, mut rows) = (String::new(), Vec::new());
        let mut wanted: Vec<(ObjectId, Role)> = Vec::new();
        let mut query = statement.query([])?;
        while let Some(row) = query.next()? {
            let path = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
            let stored = worktree_row(row, paths.len()..paths.len() + path.len(), path)?;
            if let (Some(blob), Some(role)) = (stored.blob, Role::of(path)) {
                wanted.push((blob, role));
            }
            paths.push_str(path);
            rows.push(stored);
        }
        drop(query);
        wanted.sort_unstable_by(|(blob, role), (other, other_role)| {
            order((blob, *role), (other, *other_role))
        });
        wanted.dedup();
        let mut facts = self.facts_of(&wanted)?;
        self.links_into(&mut facts)?;
        Ok(Worktree {
            paths,
            rows,
            next: 0,
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

    /// The fact of each blob and role in `wanted`, which is in the index's
    /// order, that the index holds, read in one scan of the blobs table in
    /// that order. Only a row whose blob is wanted has its other columns
    /// read, and the scan stops after the last wanted blob.
    fn facts_of(&self, wanted: &[(ObjectId, Role)]) -> Result<StoredFacts, IndexError> {
        let mut statement = self.connection().prepare_cached(
            "SELECT blob, role, kind, type, code, reason FROM main.blobs ORDER BY blob, role",
        )?;
        let mut facts = Vec::with_capacity(wanted.len());
        let mut wanted = wanted.iter().peekable();
        let mut query = statement.query([])?;
        while let Some(row) = query.next()? {
            let text = |at| row.get_ref(at)?.as_str().map_err(rusqlite::Error::from);
            let Some(blob) = ObjectId::from_hex(text(0)?) else {
                continue;
            };
            while (wanted.next_if(|(next, _)| next.as_bytes() < blob.as_bytes())).is_some() {}
            let Some((next, _)) = wanted.peek() else {
                break;
            };
            if *next != blob {
                continue;
            }
            let Some(role) = role(text(1)?) else {
                continue;
            };
            while wanted
                .next_if(|(next, as_role)| order((next, *as_role), (&blob, role)).is_lt())
                .is_some()
            {}
            if wanted.next_if_eq(&&(blob, role)).is_none() {
                continue;
            }
            if let Some(fact) = fact(row, 2)? {
                facts.push(((blob, role), fact));
            }
        }
        Ok(StoredFacts::new(facts))
    }

    /// Gives each Markdown fact in `facts` the links its blob holds, in
    /// order. One statement reads the whole table in blob and ordinal
    /// order, which costs less than finding the working tree's blobs in it.
    fn links_into(&self, facts: &mut StoredFacts) -> Result<(), IndexError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT blob, destination FROM main.links ORDER BY blob, ordinal")?;
        let mut query = statement.query([])?;
        let mut current: Option<(String, Option<usize>)> = None;
        while let Some(row) = query.next()? {
            let text = row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?;
            let at = match &current {
                Some((last, at)) if last == text => *at,
                _ => {
                    let at =
                        ObjectId::from_hex(text).and_then(|blob| facts.find(blob, Role::Markdown));
                    current = Some((text.to_owned(), at));
                    at
                }
            };
            if let Some(at) = at {
                facts.facts[at].1.links.push(row.get(1)?);
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
        let mut changed = worktree.hashed > 0;
        let mut worktree = Worktree {
            next: 0,
            ..worktree
        };
        let mut seen = vec![false; worktree.rows.len()];
        for entry in entries {
            let at = worktree.find(&entry.path);
            if let Some(at) = at {
                seen[at] = true;
            }
            if at.is_some_and(|at| worktree.rows[at].holds(entry)) {
                continue;
            }
            changed = true;
            let stat = entry.stat.as_ref();
            let code = match entry.status {
                Status::Ok => None,
                Status::Degraded(code) => Some(code),
            };
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.worktree
                 (path, logical, code, blob, size, modified_ns, changed_ns, inode, mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )?
                .execute(params![
                    entry.path,
                    entry.logical.as_ref().map(LogicalPath::as_str),
                    code.map(Code::as_str),
                    entry.blob.map(|blob| blob.to_string()),
                    stat.map(|stat| stat.size.cast_signed()),
                    stat.map(|stat| stat.modified_ns),
                    stat.map(|stat| stat.changed_ns),
                    stat.map(|stat| stat.inode.cast_signed()),
                    stat.map(|stat| stat.mode),
                ])?;
        }
        let gone = (worktree.rows.iter().zip(&seen)).filter(|(_, seen)| !**seen);
        for (row, _) in gone {
            let path = &worktree.paths[row.path.clone()];
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

/// A `worktree` row's own columns, the first nine of `row`, whose `path`
/// is kept at `at`.
fn worktree_row(
    row: &rusqlite::Row<'_>,
    at: Range<usize>,
    path: &str,
) -> rusqlite::Result<WorktreeRow> {
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
    let text = |at| {
        row.get_ref(at)?
            .as_str_or_null()
            .map_err(rusqlite::Error::from)
    };
    let logical = match text(1)? {
        Some(logical) if own(path, logical) => Logical::Own,
        logical => Logical::Stored(logical.map(str::to_owned)),
    };
    Ok(WorktreeRow {
        path: at,
        logical,
        code: text(2)?.and_then(Code::parse),
        blob: text(3)?.and_then(ObjectId::from_hex),
        stat,
    })
}

/// The role the index spells `text`.
fn role(text: &str) -> Option<Role> {
    [
        Role::Json,
        Role::Schema,
        Role::ReservedSchema,
        Role::Markdown,
    ]
    .into_iter()
    .find(|role| role.as_str() == text)
}

//! Each source's decision and mounted sources' pointer lists, kept under keys that
//! board-tree makes and this crate never reads into
//! (docs/architecture/performance/warm/decisions.md), and the views that show
//! what one run decided.

use board_model::{Change, Code, Digest, Kind, LogicalPath, Mount, ObjectId, Role, Source};
use board_tree::{Held, Probed};
use rusqlite::{OptionalExtension, params};

use crate::open::{IndexError, Storage};
use crate::schema::RESOLVED;
use crate::store::{Decided, Resolved, View, Writer, source_name};

/// The columns `fresh` and `decided` share, after the source's.
type Columns<'r> = (
    &'r str,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<&'static str>,
    Option<String>,
    &'static str,
    Option<&'static str>,
);

fn columns(entry: &Resolved) -> Columns<'_> {
    (
        &entry.path,
        entry.logical.as_ref().map(ToString::to_string),
        entry.blob.map(|blob| blob.to_string()),
        entry.size.map(u64::cast_signed),
        entry.role.map(Role::as_str),
        entry.schema.map(|schema| schema.to_string()),
        entry.kind.as_str(),
        entry.code.map(Code::as_str),
    )
}

/// One row the index keeps of a source's decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptRow {
    pub path: String,
    pub logical: Option<LogicalPath>,
    pub kind: Kind,
    pub code: Option<Code>,
    /// What check reports beyond the row, as `put_decision` was given it.
    pub detail: Option<String>,
    pub change: Change,
}

impl Decided {
    pub fn storage(&self) -> &Storage {
        self.index.storage()
    }

    /// Fills the views with the entries this run decided and with the
    /// decisions the index keeps of the sources it reached, and returns the
    /// index as a view of them.
    pub fn view(self) -> Result<View, IndexError> {
        let connection = &self.index.connection;
        // One transaction, so the inserts commit together.
        let fill = connection.unchecked_transaction()?;
        fill.execute_batch(RESOLVED)?;
        let mut statement = fill.prepare_cached(
            "INSERT INTO temp.fresh
             (source, path, logical, blob, size, role, schema, kind, code, change)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for entry in &self.entries {
            let (path, logical, blob, size, role, schema, kind, code) = columns(entry);
            let source = source_name(&entry.source);
            let change = entry.change.as_str();
            statement.execute(params![
                source, path, logical, blob, size, role, schema, kind, code, change
            ])?;
        }
        drop(statement);
        fill.commit()?;
        Ok(View::new(self.index, self.prunes))
    }
}

impl Writer {
    /// The pointers listed under `scan`, each with the mount it holds, or
    /// `None` when the index holds no such list or cannot read it.
    pub fn pointers(&self, scan: &Digest) -> Result<Option<Vec<Held>>, IndexError> {
        type Row = [Option<String>; 6];
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, logical, blob, repo, follow, commit_id FROM main.pointers
                 WHERE scan = ?1 ORDER BY path",
            )?
            .query_map([scan.hex()], |row| {
                Ok([
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ])
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        if rows.is_empty() {
            return Ok(None);
        }
        let held = rows
            .into_iter()
            .filter(|[path, ..]| path.as_deref() != Some(""))
            .map(|[path, logical, blob, repo, follow, commit]| {
                let mount = match (repo, follow, commit) {
                    (Some(repo), Some(follow), Some(commit)) => Some(Mount {
                        repo,
                        follow,
                        commit: ObjectId::from_hex(&commit)?,
                    }),
                    _ => None,
                };
                Some(Held {
                    path: path?,
                    logical: logical?.parse().ok()?,
                    blob: ObjectId::from_hex(&blob?)?,
                    mount,
                })
            })
            .collect();
        Ok(held)
    }

    /// Keeps the pointers `held` under `scan`, each with the mount it holds.
    pub fn put_pointers(&self, scan: &Digest, held: &[Held]) -> Result<(), IndexError> {
        let scan = scan.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.pointers
             (scan, path, logical, blob, repo, follow, commit_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        let none = None::<String>;
        statement.execute(params![scan, "", none, none, none, none, none])?;
        for held in held {
            let (logical, blob) = (held.logical.to_string(), held.blob.to_string());
            let mount = held.mount.as_ref();
            statement.execute(params![
                scan,
                held.path,
                logical,
                blob,
                mount.map(|mount| &mount.repo),
                mount.map(|mount| &mount.follow),
                mount.map(|mount| mount.commit.to_string()),
            ])?;
        }
        Ok(())
    }

    /// What opening a mounted repository found, kept under `stamp`, or
    /// `None` when the index holds none or cannot read it. A probe read is
    /// one this run used, so a prune keeps it.
    pub fn probe(&self, stamp: &Digest) -> Result<Option<Probed>, IndexError> {
        type Row = (String, String, Option<String>, Option<String>);
        let row: Option<Row> = self
            .connection()
            .prepare_cached("SELECT path, root, common, origin FROM main.probes WHERE stamp = ?1")?
            .query_row([stamp.hex()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?;
        let Some((path, root, common, origin)) = row else {
            return Ok(None);
        };
        let Some(root) = ObjectId::from_hex(&root) else {
            return Ok(None);
        };
        self.used(stamp)?;
        Ok(Some(Probed {
            path: path.into(),
            root,
            common: common.map(Into::into),
            origin,
        }))
    }

    /// Keeps `probed` under `stamp`. A path that is not UTF-8 is not kept.
    pub fn put_probe(&self, stamp: &Digest, probed: &Probed) -> Result<(), IndexError> {
        let (Some(path), common) = (probed.path.to_str(), probed.common.as_deref()) else {
            return Ok(());
        };
        let common = match common.map(|common| common.to_str()) {
            Some(None) => return Ok(()),
            common => common.flatten(),
        };
        self.connection()
            .prepare_cached(
                "INSERT OR REPLACE INTO main.probes (stamp, path, root, common, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                stamp.hex(),
                path,
                probed.root.to_string(),
                common,
                probed.origin
            ])?;
        self.used(stamp)
    }

    /// Records that this run used the probe under `stamp`.
    fn used(&self, stamp: &Digest) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached("INSERT OR IGNORE INTO temp.probed (stamp) VALUES (?1)")?
            .execute([stamp.hex()])?;
        Ok(())
    }

    /// Whether the index keeps a decision under `key`.
    pub fn has_decision(&self, key: &Digest) -> Result<bool, IndexError> {
        let mut statement = (self.connection())
            .prepare_cached("SELECT 1 FROM main.decided WHERE key = ?1 LIMIT 1")?;
        Ok(statement.exists([key.hex()])?)
    }

    /// Keeps `entries`, one source's whole decision, under `key`, each with
    /// its `detail`. Their source is not kept: a key names a tree, not a
    /// commit. A working-tree entry's change is kept; a mounted entry is
    /// committed.
    pub fn put_decision<'e>(
        &self,
        key: &Digest,
        entries: impl IntoIterator<Item = (&'e Resolved, Option<String>)>,
    ) -> Result<(), IndexError> {
        let key = key.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.decided
             (key, path, logical, blob, size, role, schema, kind, code, detail, change)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for (entry, detail) in entries {
            let (path, logical, blob, size, role, schema, kind, code) = columns(entry);
            let change = (entry.source == Source::WorkingTree).then(|| entry.change.as_str());
            statement.execute(params![
                key, path, logical, blob, size, role, schema, kind, code, detail, change
            ])?;
        }
        Ok(())
    }

    /// The rows kept under `key`, by path.
    pub fn decision(&self, key: &Digest) -> Result<Vec<KeptRow>, IndexError> {
        type Row = (
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, logical, kind, code, detail, change FROM main.decided
                 WHERE key = ?1 ORDER BY path",
            )?
            .query_map([key.hex()], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        rows.into_iter()
            .map(|(path, logical, kind, code, detail, change)| {
                let unreadable =
                    |logical: &Option<LogicalPath>, column, value: &str| IndexError::Unreadable {
                        path: path.clone(),
                        logical: logical.clone(),
                        column,
                        value: value.to_owned(),
                    };
                let logical = match logical {
                    Some(text) => {
                        Some((text.parse()).map_err(|_| unreadable(&None, "logical", &text))?)
                    }
                    None => None,
                };
                let kind = Kind::parse(&kind).ok_or_else(|| unreadable(&logical, "kind", &kind))?;
                let code = match code {
                    Some(text) => Some(
                        Code::parse(&text).ok_or_else(|| unreadable(&logical, "code", &text))?,
                    ),
                    None => None,
                };
                let change = match change.as_deref() {
                    None | Some("committed") => Change::Committed,
                    Some("uncommitted") => Change::Uncommitted,
                    Some(text) => return Err(unreadable(&logical, "change", text)),
                };
                Ok(KeptRow {
                    path,
                    logical,
                    kind,
                    code,
                    detail,
                    change,
                })
            })
            .collect()
    }

    /// Records that this run reached `source`, whose decision the index
    /// keeps under `key` when there is one, and whose pointers it lists
    /// under `scan` when it is a mounted source.
    pub fn reach(
        &self,
        source: &Source,
        key: Option<&Digest>,
        scan: Option<&Digest>,
    ) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached(
                "INSERT OR REPLACE INTO temp.reached (source, key, scan) VALUES (?1, ?2, ?3)",
            )?
            .execute(params![
                source_name(source),
                key.map(Digest::hex),
                scan.map(Digest::hex)
            ])?;
        Ok(())
    }
}

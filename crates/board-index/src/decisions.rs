//! Mounted sources' decisions and pointer lists, kept under keys that
//! board-tree makes and this crate never reads into
//! (docs/architecture/performance/decisions.md), and the views that show
//! what one run decided.

use board_model::{Code, Digest, Kind, LogicalPath, ObjectId, Role, Source};
use board_tree::Held;
use rusqlite::params;

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
        Ok(View {
            index: self.index,
            prunes: self.prunes,
        })
    }
}

impl Writer {
    /// The pointers listed under `scan`, or `None` when the index holds no
    /// such list or cannot read it.
    pub fn pointers(&self, scan: &Digest) -> Result<Option<Vec<Held>>, IndexError> {
        type Row = (String, Option<String>, Option<String>);
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, logical, blob FROM main.pointers WHERE scan = ?1 ORDER BY path",
            )?
            .query_map([scan.hex()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        if rows.is_empty() {
            return Ok(None);
        }
        let held = rows
            .into_iter()
            .filter(|(path, _, _)| !path.is_empty())
            .map(|(path, logical, blob)| {
                Some(Held {
                    path,
                    logical: logical?.parse().ok()?,
                    blob: ObjectId::from_hex(&blob?)?,
                })
            })
            .collect();
        Ok(held)
    }

    pub fn put_pointers(&self, scan: &Digest, held: &[Held]) -> Result<(), IndexError> {
        let scan = scan.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.pointers (scan, path, logical, blob)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        statement.execute(params![scan, "", None::<String>, None::<String>])?;
        for held in held {
            let (logical, blob) = (held.logical.to_string(), held.blob.to_string());
            statement.execute(params![scan, held.path, logical, blob])?;
        }
        Ok(())
    }

    /// Whether the index keeps a decision under `key`.
    pub fn has_decision(&self, key: &Digest) -> Result<bool, IndexError> {
        let mut statement = (self.connection())
            .prepare_cached("SELECT 1 FROM main.decided WHERE key = ?1 LIMIT 1")?;
        Ok(statement.exists([key.hex()])?)
    }

    /// Keeps `entries`, one source's whole decision, under `key`, each with
    /// its `detail`. Their source and change are not kept: a key names a
    /// tree, not a commit, and a mounted entry is committed.
    pub fn put_decision<'e>(
        &self,
        key: &Digest,
        entries: impl IntoIterator<Item = (&'e Resolved, Option<String>)>,
    ) -> Result<(), IndexError> {
        let key = key.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.decided
             (key, path, logical, blob, size, role, schema, kind, code, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for (entry, detail) in entries {
            let (path, logical, blob, size, role, schema, kind, code) = columns(entry);
            statement.execute(params![
                key, path, logical, blob, size, role, schema, kind, code, detail
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
        );
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, logical, kind, code, detail FROM main.decided
                 WHERE key = ?1 ORDER BY path",
            )?
            .query_map([key.hex()], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        rows.into_iter()
            .map(|(path, logical, kind, code, detail)| {
                let unreadable = |column, value: &str| IndexError::Unreadable {
                    path: path.clone(),
                    column,
                    value: value.to_owned(),
                };
                let logical = match logical {
                    Some(text) => Some(text.parse().map_err(|_| unreadable("logical", &text))?),
                    None => None,
                };
                let kind = Kind::parse(&kind).ok_or_else(|| unreadable("kind", &kind))?;
                let code = match code {
                    Some(text) => {
                        Some(Code::parse(&text).ok_or_else(|| unreadable("code", &text))?)
                    }
                    None => None,
                };
                Ok(KeptRow {
                    path,
                    logical,
                    kind,
                    code,
                    detail,
                })
            })
            .collect()
    }

    /// Records that this run reached the mounted `source`, whose decision
    /// the index keeps under `key` when there is one, and whose pointers it
    /// lists under `scan`.
    pub fn reach(
        &self,
        source: &Source,
        key: Option<&Digest>,
        scan: &Digest,
    ) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached(
                "INSERT OR REPLACE INTO temp.reached (source, key, scan) VALUES (?1, ?2, ?3)",
            )?
            .execute(params![
                source_name(source),
                key.map(Digest::hex),
                scan.hex()
            ])?;
        Ok(())
    }
}

//! Each source's decision and mounted sources' pointer lists, kept under keys that
//! board-tree makes and this crate never reads into
//! (docs/architecture/performance/warm/decisions.md), and the views that show
//! what one run decided.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

use board_model::{
    Board, Change, Code, Digest, Kind, Location, LogicalPath, Mount, Object, ObjectId, Role, Source,
};
use board_tree::{Held, Probed};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

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
    pub blob: Option<ObjectId>,
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
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT path, logical, blob, kind, code, detail, change FROM main.decided
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
                    row.get(6)?,
                ))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        rows.into_iter()
            .map(|(path, logical, blob, kind, code, detail, change)| {
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
                let blob = match blob {
                    Some(text) => Some(
                        ObjectId::from_hex(&text)
                            .ok_or_else(|| unreadable(&logical, "blob", &text))?,
                    ),
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
                    blob,
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

/// One reference a record or Markdown file makes, and where it led.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub pointer: String,
    /// Whether it is a Markdown link, whose `pointer` is its destination.
    pub link: bool,
    pub target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub location: Option<Location>,
    pub code: Option<Code>,
}

/// One reference an entry of a kept decision makes: the entry's path in
/// its source, its logical path as its kept row holds it, and the reference.
pub type KeptReference = (String, Option<LogicalPath>, Reached);

impl Writer {
    /// Keeps under `key` each reference `references` gives, by the path in
    /// `source` of the entry that makes it, numbered in order for each
    /// path. A location in `source` itself is kept without its source,
    /// since a key names a tree, not a commit.
    pub fn put_references<'r>(
        &self,
        key: &Digest,
        source: &Source,
        references: impl IntoIterator<Item = (&'r str, &'r Reached)>,
    ) -> Result<(), IndexError> {
        let key = key.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.resolutions
             (key, path, ordinal, pointer, link, target, location, code)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut last: Option<(&str, i64)> = None;
        for (path, reached) in references {
            let ordinal = match last {
                Some((was, ordinal)) if was == path => ordinal + 1,
                _ => 0,
            };
            last = Some((path, ordinal));
            let location = (reached.location.as_ref()).map(|at| encoded(at, source).to_string());
            statement.execute(params![
                key,
                path,
                ordinal,
                reached.pointer,
                reached.link,
                reached.target.as_str(),
                location,
                reached.code.map(Code::as_str),
            ])?;
        }
        Ok(())
    }

    /// Each reference kept under `key`, by path and then in order, with
    /// each location kept without its source given `source`.
    pub fn references(
        &self,
        key: &Digest,
        source: &Source,
    ) -> Result<Vec<KeptReference>, IndexError> {
        type Row = (
            String,
            Option<String>,
            String,
            bool,
            String,
            Option<String>,
            Option<String>,
        );
        let rows = self
            .connection()
            .prepare_cached(
                "SELECT r.path, d.logical, r.pointer, r.link, r.target, r.location, r.code
                 FROM main.resolutions AS r
                 LEFT JOIN main.decided AS d ON d.key = r.key AND d.path = r.path
                 WHERE r.key = ?1 ORDER BY r.path, r.ordinal",
            )?
            .query_map([key.hex()], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .collect::<Result<Vec<Row>, _>>()?;
        rows.into_iter()
            .map(|(path, logical, pointer, link, target, location, code)| {
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
                let target =
                    (target.parse()).map_err(|_| unreadable(&logical, "target", &target))?;
                let location = match location {
                    Some(text) => Some(
                        (serde_json::from_str(&text).ok())
                            .and_then(|value| decoded(&value, source))
                            .ok_or_else(|| unreadable(&logical, "location", &text))?,
                    ),
                    None => None,
                };
                let code = match code {
                    Some(text) => Some(
                        Code::parse(&text).ok_or_else(|| unreadable(&logical, "code", &text))?,
                    ),
                    None => None,
                };
                let reached = Reached {
                    pointer,
                    link,
                    target,
                    location,
                    code,
                };
                Ok((path, logical, reached))
            })
            .collect()
    }
}

/// `location` as JSON, its source `null` where it is `own`.
fn encoded(location: &Location, own: &Source) -> Value {
    let Board { repository, source } = &location.board;
    let repository = match repository.to_str() {
        Some(text) => json!(text),
        None => json!(repository.as_os_str().as_bytes()),
    };
    let source = match source {
        source if source == own => Value::Null,
        Source::WorkingTree => json!("worktree"),
        Source::Commit(commit) => json!(commit.to_string()),
    };
    let object = match location.object {
        Object::Blob(blob) => json!(blob.to_string()),
        Object::Directory => Value::Null,
    };
    let pointers: Vec<Value> = (location.pointers.iter())
        .map(|pointer| encoded(pointer, own))
        .collect();
    json!({
        "repository": repository,
        "source": source,
        "path": location.path,
        "object": object,
        "pointers": pointers,
        "follow": location.follow,
    })
}

/// The location [`encoded`] gave as `value`, or `None` when it reads as none.
fn decoded(value: &Value, own: &Source) -> Option<Location> {
    let repository = match value.get("repository")? {
        Value::String(text) => PathBuf::from(text),
        Value::Array(bytes) => {
            let bytes: Option<Vec<u8>> = (bytes.iter())
                .map(|byte| u8::try_from(byte.as_u64()?).ok())
                .collect();
            PathBuf::from(OsStr::from_bytes(&bytes?))
        }
        _ => return None,
    };
    let source = match value.get("source")? {
        Value::Null => own.clone(),
        Value::String(text) if text == "worktree" => Source::WorkingTree,
        Value::String(text) => Source::Commit(ObjectId::from_hex(text)?),
        _ => return None,
    };
    let object = match value.get("object")? {
        Value::Null => Object::Directory,
        Value::String(text) => Object::Blob(ObjectId::from_hex(text)?),
        _ => return None,
    };
    let pointers = (value.get("pointers")?.as_array()?.iter())
        .map(|pointer| decoded(pointer, own))
        .collect::<Option<Vec<Location>>>()?;
    let follow = match value.get("follow")? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        _ => return None,
    };
    Some(Location {
        board: Board { repository, source },
        path: value.get("path")?.as_str()?.to_owned(),
        object,
        pointers,
        follow,
    })
}

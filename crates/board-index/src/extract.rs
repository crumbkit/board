//! What each plugin version has extracted, and which records it still needs
//! (docs/plugins.md, Extraction). Call these after a refresh has decided the
//! current entries.

use std::path::{Path, PathBuf};

use board_model::{ObjectId, Source};
use rusqlite::params;

use crate::open::{Index, IndexError};

/// A record a plugin has not yet handled: one route to its blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unhandled {
    pub blob: ObjectId,
    pub source: Source,
    /// The path inside its source.
    pub path: String,
    /// The display path from the root board.
    pub logical: String,
    pub record_type: String,
    /// The repository its source was read from.
    pub repository: PathBuf,
}

impl Index {
    /// Each current record of one of `types` whose blob parses and validates
    /// through some route, and which `plugin` at `version` has not handled,
    /// once per blob by its first display path, in display path order.
    pub fn unhandled(
        &self,
        plugin: &str,
        version: &str,
        types: &[&str],
    ) -> Result<Vec<Unhandled>, IndexError> {
        let types = serde_json::Value::from(types).to_string();
        let mut statement = self.connection.prepare(
            "SELECT r.blob, r.source, r.path, min(coalesce(r.logical, r.path)), b.type,
                    s.repository
             FROM temp.resolved AS r
             JOIN main.sources AS s ON s.source = r.source
             JOIN main.blobs AS b ON b.blob = r.blob AND b.role = r.role
             JOIN main.validations AS v ON v.record = r.blob AND v.schema = r.schema
             WHERE r.kind = 'record' AND b.code IS NULL AND v.code IS NULL
               AND b.type IN (SELECT value FROM json_each(?3))
               AND r.blob NOT IN (
                   SELECT blob FROM main.extracted WHERE plugin = ?1 AND version = ?2
               )
             GROUP BY r.blob
             ORDER BY 4, r.blob",
        )?;
        let rows = statement.query_map(params![plugin, version, types], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut unhandled = Vec::new();
        for row in rows {
            let (blob, source, path, logical, record_type, repository) = row?;
            let (Some(blob), Some(source)) = (ObjectId::from_hex(&blob), source_of(&source)) else {
                return Err(IndexError::Foreign);
            };
            unhandled.push(Unhandled {
                blob,
                source,
                path,
                logical,
                record_type,
                repository: PathBuf::from(repository),
            });
        }
        Ok(unhandled)
    }

    /// Records that `plugin` at `version` handled each of `blobs`.
    pub fn handled(
        &self,
        plugin: &str,
        version: &str,
        blobs: &[ObjectId],
    ) -> Result<(), IndexError> {
        let mut insert = self.connection.prepare(
            "INSERT OR IGNORE INTO main.extracted (plugin, version, blob) VALUES (?1, ?2, ?3)",
        )?;
        for blob in blobs {
            insert.execute(params![plugin, version, blob.to_string()])?;
        }
        Ok(())
    }

    /// Forgets every blob `plugin` at `version` handled, as when its
    /// database is gone.
    pub fn forget(&self, plugin: &str, version: &str) -> Result<(), IndexError> {
        self.connection.execute(
            "DELETE FROM main.extracted WHERE plugin = ?1 AND version = ?2",
            params![plugin, version],
        )?;
        Ok(())
    }

    /// Attaches the `SQLite` file at `path`, read-only, as `name`, which must
    /// be a plain identifier.
    pub fn attach(&self, name: &str, path: &Path) -> Result<(), IndexError> {
        let escaped = path
            .to_string_lossy()
            .replace('%', "%25")
            .replace('?', "%3F")
            .replace('#', "%23");
        let uri = format!("file:{escaped}?mode=ro");
        self.connection
            .execute("ATTACH DATABASE ?1 AS ?2", params![uri, name])?;
        Ok(())
    }
}

/// The source the index names `name`.
fn source_of(name: &str) -> Option<Source> {
    if name == "worktree" {
        return Some(Source::WorkingTree);
    }
    ObjectId::from_hex(name).map(Source::Commit)
}

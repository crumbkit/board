//! What each plugin version has extracted, and which records it still needs
//! (docs/plugins/extraction.md).

use std::path::{Path, PathBuf};

use board_model::{ObjectId, Source};
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::open::IndexError;
use crate::store::View;

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

impl View {
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
        let mut statement = self.connection().prepare(
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

    /// Records that `plugin` at `version` handled each of `blobs` into the
    /// database with this `id`, unless the index now trusts another.
    pub fn handled(
        &self,
        plugin: &str,
        version: &str,
        id: &str,
        blobs: &[ObjectId],
    ) -> Result<(), IndexError> {
        let mut insert = self.connection().prepare(
            "INSERT OR IGNORE INTO main.extracted (plugin, version, blob)
             SELECT ?1, ?2, ?3 WHERE (SELECT value FROM main.state WHERE key = ?4) = ?5",
        )?;
        let key = database(plugin, version);
        for blob in blobs {
            insert.execute(params![plugin, version, blob.to_string(), key, id])?;
        }
        Ok(())
    }

    /// Makes what `plugin` at `version` handled describe its database with
    /// this `id`. Unless the index recorded that id for it, the index forgets
    /// every blob it handled and records the id. Trusting the recorded id only
    /// reads, so it does not wait for another process's write.
    pub fn trust(&self, plugin: &str, version: &str, id: &str) -> Result<(), IndexError> {
        let key = database(plugin, version);
        let recorded = |connection: &Connection| {
            connection
                .query_row(
                    "SELECT value FROM main.state WHERE key = ?1",
                    [&key],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        };
        if recorded(self.connection())?.as_deref() == Some(id) {
            return Ok(());
        }
        let transaction =
            Transaction::new_unchecked(self.connection(), TransactionBehavior::Immediate)?;
        if recorded(&transaction)?.as_deref() != Some(id) {
            transaction.execute(
                "DELETE FROM main.extracted WHERE plugin = ?1 AND version = ?2",
                params![plugin, version],
            )?;
            transaction.execute(
                "INSERT OR REPLACE INTO main.state (key, value) VALUES (?1, ?2)",
                params![key, id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Attaches the `SQLite` file at `path`, read-only, as `name`, which must
    /// be a plain identifier. `SQLite` refuses a file that is not a database.
    pub fn attach(&self, name: &str, path: &Path) -> Result<(), IndexError> {
        let escaped = path
            .to_string_lossy()
            .replace('%', "%25")
            .replace('?', "%3F")
            .replace('#', "%23");
        let uri = format!("file:{escaped}?mode=ro");
        self.connection()
            .execute("ATTACH DATABASE ?1 AS ?2", params![uri, name])?;
        Ok(())
    }
}

/// The key in `state` of the id of `plugin`'s database at `version`.
fn database(plugin: &str, version: &str) -> String {
    format!("database {plugin}@{version}")
}

/// The source the index names `name`.
fn source_of(name: &str) -> Option<Source> {
    if name == "worktree" {
        return Some(Source::WorkingTree);
    }
    ObjectId::from_hex(name).map(Source::Commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_named_worktree_or_by_its_commit_and_any_other_name_is_none() {
        assert_eq!(source_of("worktree"), Some(Source::WorkingTree));
        let commit = ObjectId::Sha1([0xab; 20]);
        assert_eq!(source_of(&commit.to_string()), Some(Source::Commit(commit)));
        assert_eq!(source_of("Worktree"), None);
    }
}

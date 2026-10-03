//! Refreshing the index from the working tree (docs/architecture.md, The
//! index). Only files whose metadata changed are hashed, and every fact is
//! keyed by content, so a blob already indexed is not read again and a
//! record is validated again only against a schema blob it has not met.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use board_index::{BlobFact, Index, IndexError, RefFact, ValidationFact, WorktreeRow, Writer};
use board_model::{Classified, Code, Content, ObjectId, Role, Source, Stat};
use board_tree::{Entry, GitError, Repository, Status};
use board_validate::Schema;
use serde_json::Value;

use crate::board::Board;

/// The work a refresh did: files hashed, blobs parsed and records
/// validated. Work-count tests assert these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    pub hashed: usize,
    pub parsed: usize,
    pub validated: usize,
}

/// Parsed values from this refresh, so validation need not read them again.
type Values = HashMap<(ObjectId, Role), Value>;

impl Board {
    /// Brings `index` up to date with the working tree, adding every fact it
    /// does not yet hold.
    pub fn refresh_index(&self, index: &mut Index) -> Result<Refreshed, RefreshError> {
        let started_ns = now_ns();
        let writer = index.write()?;
        let mut refreshed = Refreshed::default();
        let entries = self.walk(&writer, &mut refreshed.hashed)?;
        let mut values = Values::new();
        let mut facts: Vec<(&Entry, ObjectId, Role, BlobFact)> = Vec::new();
        for entry in &entries {
            let (Some(blob), Some(role)) = (entry.blob, Role::of(&entry.path)) else {
                continue;
            };
            let fact = if let Some(fact) = writer.blob(blob, role)? {
                fact
            } else {
                // A blob that cannot be read has no content fact to store.
                let Ok(bytes) = self
                    .repository()
                    .read(&Source::WorkingTree, &entry.path, blob)
                else {
                    continue;
                };
                let classified = role.classify(bytes.as_deref());
                let fact = blob_fact(&classified);
                writer.put_blob(blob, role, &fact)?;
                refreshed.parsed += 1;
                if let Some(value) = value(classified) {
                    values.insert((blob, role), value);
                }
                fact
            };
            facts.push((entry, blob, role, fact));
        }
        refreshed.validated = self.validate(&writer, &facts, &values)?;
        writer.set_refreshed_ns(started_ns)?;
        writer.commit()?;
        Ok(refreshed)
    }

    /// Walks the working tree, hashing only files whose metadata the index
    /// does not hold, and records what it saw.
    fn walk(&self, writer: &Writer<'_>, hashed: &mut usize) -> Result<Vec<Entry>, RefreshError> {
        let clean_before_ns = writer.refreshed_ns()?.map(whole_second);
        let stored = writer.worktree()?;
        let entries = self.repository().walk_working_tree(&mut |path, stat| {
            let known = stored
                .get(path)
                .filter(|row| row.stat.as_ref() == Some(stat) && clean(stat, clean_before_ns))
                .and_then(|row| row.blob);
            if known.is_none() {
                *hashed += 1;
            }
            known
        })?;
        for entry in &entries {
            let row = WorktreeRow {
                logical: entry.logical.clone(),
                code: match entry.status {
                    Status::Ok => None,
                    Status::Degraded(code) => Some(code),
                },
                blob: entry.blob,
                stat: entry.stat,
            };
            if stored.get(&entry.path) != Some(&row) {
                writer.put_worktree(&entry.path, &row)?;
            }
        }
        let walked: std::collections::HashSet<&str> =
            entries.iter().map(|entry| entry.path.as_str()).collect();
        for path in stored.keys().filter(|path| !walked.contains(path.as_str())) {
            writer.delete_worktree(path)?;
        }
        Ok(entries)
    }

    /// Validates each record against its schema, unless the index already
    /// holds that pair, and returns how many it validated.
    fn validate(
        &self,
        writer: &Writer<'_>,
        facts: &[(&Entry, ObjectId, Role, BlobFact)],
        values: &Values,
    ) -> Result<usize, RefreshError> {
        let schemas: HashMap<&str, ObjectId> = facts
            .iter()
            .filter(|(_, _, role, fact)| *role == Role::Schema && fact.code.is_none())
            .map(|(entry, blob, _, _)| (entry.path.as_str(), *blob))
            .collect();
        let mut compiled: HashMap<ObjectId, Option<Schema>> = HashMap::new();
        let mut validated = 0;
        for (entry, record, _, fact) in facts {
            let Some(record_type) = fact.record_type.as_ref() else {
                continue;
            };
            let schema_path = record_type.schema_path();
            let Some(&schema) = schemas.get(schema_path.as_str()) else {
                continue;
            };
            if fact.code.is_some()
                || record_type.is_board()
                || writer.validation(*record, schema)?.is_some()
            {
                continue;
            }
            let repository = self.repository();
            let Some(schema_value) = compiled
                .entry(schema)
                .or_insert_with(|| {
                    known(values, repository, &schema_path, schema, Role::Schema)
                        .and_then(|value| Schema::compile(value).ok())
                })
                .as_ref()
            else {
                continue;
            };
            let Some(value) = known(values, repository, &entry.path, *record, Role::Json) else {
                continue;
            };
            writer.put_validation(*record, schema, &validate(schema_value, &value))?;
            validated += 1;
        }
        Ok(validated)
    }
}

/// Whether a file whose metadata is unchanged can be trusted to be
/// unchanged: only if it was last modified before the refresh that saw it
/// began. Like git's racy-clean rule, this compares whole seconds, so a file
/// system with one-second times cannot hide a change made in the same second.
fn clean(stat: &Stat, clean_before_ns: Option<i64>) -> bool {
    clean_before_ns.is_some_and(|before| stat.modified_ns < before)
}

fn whole_second(ns: i64) -> i64 {
    ns - ns.rem_euclid(1_000_000_000)
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
        })
}

/// What the index keeps of a classification. A schema that parses is also
/// compiled, since whether it compiles depends on its bytes alone.
fn blob_fact(classified: &Classified) -> BlobFact {
    let code = classified.code.or_else(|| match &classified.content {
        Content::Schema { value } => Schema::compile(value.clone())
            .err()
            .map(|error| error.code()),
        _ => None,
    });
    BlobFact {
        kind: classified.kind(),
        record_type: match &classified.content {
            Content::Record { record_type, .. } => Some(record_type.clone()),
            _ => None,
        },
        code,
    }
}

fn value(classified: Classified) -> Option<Value> {
    match classified.content {
        Content::Record { value, .. } | Content::Schema { value } => Some(value),
        _ => None,
    }
}

/// The parsed value of a blob, from this build or read again.
fn known(
    values: &HashMap<(ObjectId, Role), Value>,
    repository: &Repository,
    path: &str,
    blob: ObjectId,
    role: Role,
) -> Option<Value> {
    if let Some(value) = values.get(&(blob, role)) {
        return Some(value.clone());
    }
    let bytes = repository.read(&Source::WorkingTree, path, blob).ok()??;
    value(role.classify(Some(&bytes)))
}

fn validate(schema: &Schema, record: &Value) -> ValidationFact {
    match schema.validate(record) {
        Ok(()) => ValidationFact::Valid(
            schema
                .references(record)
                .into_iter()
                .map(|reference| RefFact {
                    pointer: reference.pointer,
                    target: reference.path,
                    accepts: reference.accepts,
                })
                .collect(),
        ),
        Err(error) => ValidationFact::Invalid {
            code: error.code(),
            pointer: match error {
                board_validate::RecordError::Invalid { pointer, .. }
                | board_validate::RecordError::MalformedReference { pointer } => pointer,
            },
        },
    }
}

/// A refresh that could not finish.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Git {
        #[from]
        source: GitError,
    },
    #[error(transparent)]
    Index {
        #[from]
        source: IndexError,
    },
}

impl RefreshError {
    /// The code of a git failure; an index failure has none yet.
    pub fn code(&self) -> Option<Code> {
        match self {
            Self::Git { source } => Some(source.code()),
            Self::Index { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;

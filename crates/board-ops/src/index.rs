//! Filling the index from the working tree (docs/architecture.md, Index).
//! Every fact is keyed by content, so a blob already indexed is not read
//! again, and a record is validated again only against a schema blob it has
//! not met.

use std::collections::HashMap;

use board_index::{BlobFact, Index, IndexError, RefFact, ValidationFact};
use board_model::{Classified, Code, Content, ObjectId, Role, Source};
use board_tree::{Entry, GitError, Repository};
use board_validate::Schema;
use serde_json::Value;

use crate::board::Board;

/// The work a build did: blobs parsed and records validated. Work-count
/// tests assert these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Built {
    pub parsed: usize,
    pub validated: usize,
}

/// A record found in the walk, with the schema it needs.
struct Pending {
    record: ObjectId,
    path: String,
    schema: ObjectId,
}

impl Board {
    /// Adds to `index` every fact about the working tree it does not yet
    /// hold.
    pub fn build_index(&self, index: &mut Index) -> Result<Built, BuildError> {
        let source = Source::WorkingTree;
        let repository = self.repository();
        let entries = repository.walk(&source)?;
        let writer = index.write()?;
        let mut built = Built::default();
        let mut values: HashMap<(ObjectId, Role), Value> = HashMap::new();
        let mut facts: Vec<(&Entry, ObjectId, Role, BlobFact)> = Vec::new();
        for entry in &entries {
            let (Some(blob), Some(role)) = (entry.blob, Role::of(&entry.path)) else {
                continue;
            };
            let fact = if let Some(fact) = writer.blob(blob, role)? {
                fact
            } else {
                // A blob that cannot be read has no content fact to store.
                let Ok(bytes) = repository.read(&source, &entry.path, blob) else {
                    continue;
                };
                let classified = role.classify(bytes.as_deref());
                let fact = blob_fact(&classified);
                writer.put_blob(blob, role, &fact)?;
                built.parsed += 1;
                if let Some(value) = value(classified) {
                    values.insert((blob, role), value);
                }
                fact
            };
            facts.push((entry, blob, role, fact));
        }
        let schemas: HashMap<&str, ObjectId> = facts
            .iter()
            .filter(|(_, _, role, fact)| *role == Role::Schema && fact.code.is_none())
            .map(|(entry, blob, _, _)| (entry.path.as_str(), *blob))
            .collect();
        let pending: Vec<Pending> = facts
            .iter()
            .filter_map(|(entry, blob, _, fact)| {
                let record_type = fact.record_type.as_ref()?;
                let schema = *schemas.get(record_type.schema_path().as_str())?;
                (fact.code.is_none() && !record_type.is_board()).then(|| Pending {
                    record: *blob,
                    path: entry.path.clone(),
                    schema,
                })
            })
            .collect();
        let mut compiled: HashMap<ObjectId, Option<Schema>> = HashMap::new();
        for pending in pending {
            if writer.validation(pending.record, pending.schema)?.is_some() {
                continue;
            }
            let schema = compiled.entry(pending.schema).or_insert_with(|| {
                let path = schemas
                    .iter()
                    .find(|(_, blob)| **blob == pending.schema)
                    .map(|(path, _)| *path)?;
                let value = known(&values, repository, path, pending.schema, Role::Schema)?;
                Schema::compile(value).ok()
            });
            let Some(schema) = schema else { continue };
            let Some(record) = known(
                &values,
                repository,
                &pending.path,
                pending.record,
                Role::Json,
            ) else {
                continue;
            };
            writer.put_validation(pending.record, pending.schema, &validate(schema, &record))?;
            built.validated += 1;
        }
        writer.commit()?;
        Ok(built)
    }
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

/// A build that could not finish.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
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

impl BuildError {
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

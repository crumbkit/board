//! Refreshing the index from the working tree (docs/architecture/index.md).
//! Only files whose metadata changed are hashed, and every fact is keyed by
//! content, so a blob already indexed is not read again and a record is
//! validated again only against a schema blob it has not met.
//! A refresh that finds nothing changed writes nothing.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use board_index::{BlobFact, Index, IndexError, RefFact, ValidationFact, WorktreeRow, Writer};
use board_model::{
    CONFIG_TYPE, Classified, Code, Content, MANIFEST_TYPE, ObjectId, Role, Source, Stat,
};
use board_plugin::Manifest;
use board_tree::{Entry, GitError, Repository, Status};
use board_validate::Schema;
use serde_json::Value;

use crate::board::{Board, Config};

/// The work a refresh did: files hashed, blobs parsed and records
/// validated. Work-count tests assert these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Trees of `HEAD` listed.
    pub trees: usize,
    pub hashed: usize,
    pub parsed: usize,
    pub validated: usize,
    /// Whether it took the index's write lock.
    pub wrote: bool,
    /// Rows a prune deleted.
    pub pruned: usize,
}

/// Parsed values from this refresh, so validation need not read them again.
type Values = HashMap<(ObjectId, Role), Value>;

/// One entry's blob, the role its path gives it, and what the index holds
/// about it.
#[derive(Clone, Debug)]
pub(crate) struct Indexed {
    pub(crate) blob: ObjectId,
    pub(crate) role: Role,
    pub(crate) fact: BlobFact,
}

/// What a refresh found, for a check to decide each entry's status.
#[derive(Debug)]
pub(crate) struct Refresh {
    pub(crate) entries: Vec<Entry>,
    /// The fact for each entry that has one, by physical path.
    pub(crate) facts: HashMap<String, Indexed>,
    /// What `HEAD` holds at each path; empty before the first commit.
    pub(crate) head: HashMap<String, Option<ObjectId>>,
    pub(crate) work: Refreshed,
}

impl Board {
    /// Brings `index` up to date with the working tree, adding every fact it
    /// does not yet hold.
    pub fn refresh_index(&self, index: &mut Index) -> Result<Refreshed, RefreshError> {
        let (work, wrote) = transact(index, |writer| Ok(self.refresh(writer)?.work))?;
        Ok(Refreshed { wrote, ..work })
    }

    pub(crate) fn refresh(&self, writer: &Writer<'_>) -> Result<Refresh, RefreshError> {
        let started_ns = now_ns();
        let mut work = Refreshed::default();
        let (entries, changed) = self.walk(writer, &mut work.hashed)?;
        let facts = facts(
            writer,
            self.repository(),
            &Source::WorkingTree,
            &entries,
            &mut work,
        )?;
        let head = self.head(writer, &mut work.trees)?;
        // Keeping the older start time trusts no file the stored one did not.
        if changed {
            writer.set_refreshed_ns(started_ns)?;
        }
        Ok(Refresh {
            entries,
            facts,
            head,
            work,
        })
    }

    /// Adds `HEAD`'s trees the index does not hold, and returns its files.
    fn head(
        &self,
        writer: &Writer<'_>,
        listed: &mut usize,
    ) -> Result<HashMap<String, Option<ObjectId>>, RefreshError> {
        let repository = self.repository();
        let Some(root) = repository.head_tree()? else {
            return Ok(HashMap::new());
        };
        let mut known = |tree| writer.has_tree(tree).unwrap_or(false);
        for tree in repository.trees(root, &mut known)? {
            writer.put_tree(&tree)?;
            *listed += 1;
        }
        Ok(writer.files(root)?)
    }

    /// Walks the working tree, hashing only files whose metadata the index
    /// does not hold, and records what it saw. Returns the entries and
    /// whether it hashed or recorded anything.
    fn walk(
        &self,
        writer: &Writer<'_>,
        hashed: &mut usize,
    ) -> Result<(Vec<Entry>, bool), RefreshError> {
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
        let mut changed = *hashed > 0;
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
                changed = true;
            }
        }
        let walked: std::collections::HashSet<&str> =
            entries.iter().map(|entry| entry.path.as_str()).collect();
        for path in stored.keys().filter(|path| !walked.contains(path.as_str())) {
            writer.delete_worktree(path)?;
            changed = true;
        }
        Ok((entries, changed))
    }
}

/// Runs `run` in a transaction that takes the write lock only when it
/// writes, so that runs finding nothing changed do not wait on each other,
/// and returns what it returned and whether it wrote. If another process
/// holds the lock or wrote first, it runs again in a transaction that waits
/// for the lock. If the wait runs out, it runs once more in an index in
/// memory, as opening does.
pub(crate) fn transact<T>(
    index: &mut Index,
    mut run: impl FnMut(&Writer<'_>) -> Result<T, RefreshError>,
) -> Result<(T, bool), RefreshError> {
    let first = once(index.deferred(), &mut run);
    if !busy(&first) {
        return first;
    }
    let second = once(index.write(), &mut run);
    if !busy(&second) {
        return second;
    }
    index.fall_back()?;
    once(index.write(), &mut run)
}

fn once<T>(
    writer: Result<Writer<'_>, IndexError>,
    run: &mut impl FnMut(&Writer<'_>) -> Result<T, RefreshError>,
) -> Result<(T, bool), RefreshError> {
    let writer = writer?;
    let value = run(&writer)?;
    let wrote = writer.wrote()?;
    writer.commit()?;
    Ok((value, wrote))
}

/// Whether `result` failed because another process held the index.
fn busy<T>(result: &Result<T, RefreshError>) -> bool {
    matches!(
        result,
        Err(RefreshError::Index {
            source: IndexError::Busy { .. }
        })
    )
}

/// What the index holds about each entry of `source` in `repository`,
/// parsing each blob and validating each record it does not hold yet.
pub(crate) fn facts(
    writer: &Writer<'_>,
    repository: &Repository,
    source: &Source,
    entries: &[Entry],
    work: &mut Refreshed,
) -> Result<HashMap<String, Indexed>, RefreshError> {
    let mut values = Values::new();
    let mut facts: HashMap<String, Indexed> = HashMap::new();
    for entry in entries {
        let (Some(blob), Some(role)) = (entry.blob, Role::of(&entry.path)) else {
            continue;
        };
        let fact = if let Some(fact) = writer.blob(blob, role)? {
            fact
        } else {
            // A blob that cannot be read has no content fact to store.
            let Ok(bytes) = repository.read(source, &entry.path, blob) else {
                continue;
            };
            let classified = role.classify(bytes.as_deref());
            let fact = blob_fact(&classified);
            writer.put_blob(blob, role, &fact)?;
            if let Content::Record { value, .. } = &classified.content {
                writer.put_body(blob, value)?;
            }
            work.parsed += 1;
            if let Some(value) = value(classified) {
                values.insert((blob, role), value);
            }
            fact
        };
        facts.insert(entry.path.clone(), Indexed { blob, role, fact });
    }
    work.validated += validate_all(writer, repository, source, &facts, &values)?;
    Ok(facts)
}

/// Validates each record against its schema, unless the index already
/// holds that pair, and returns how many it validated.
fn validate_all(
    writer: &Writer<'_>,
    repository: &Repository,
    source: &Source,
    facts: &HashMap<String, Indexed>,
    values: &Values,
) -> Result<usize, RefreshError> {
    let mut compiled: HashMap<ObjectId, Option<Schema>> = HashMap::new();
    let mut validated = 0;
    for (path, record) in facts {
        let Some(schema_path) = schema_path(record) else {
            continue;
        };
        let Some(schema) = schema_of(facts, &schema_path) else {
            continue;
        };
        if writer.validation(record.blob, schema)?.is_some() {
            continue;
        }
        let Some(compiled) = compiled
            .entry(schema)
            .or_insert_with(|| {
                known(
                    values,
                    repository,
                    source,
                    &schema_path,
                    schema,
                    Role::Schema,
                )
                .and_then(|value| Schema::compile(value).ok())
            })
            .as_ref()
        else {
            continue;
        };
        let Some(value) = known(values, repository, source, path, record.blob, Role::Json) else {
            continue;
        };
        writer.put_validation(record.blob, schema, &validate(compiled, &value))?;
        validated += 1;
    }
    Ok(validated)
}

/// The schema path of a record that is checked against a schema: one that
/// parsed, with a type outside the `board` namespace.
pub(crate) fn schema_path(record: &Indexed) -> Option<String> {
    let record_type = record.fact.record_type.as_ref()?;
    (record.fact.code.is_none() && !record_type.is_board()).then(|| record_type.schema_path())
}

/// The blob of the usable schema at `path`.
pub(crate) fn schema_of(facts: &HashMap<String, Indexed>, path: &str) -> Option<ObjectId> {
    facts
        .get(path)
        .filter(|schema| schema.role == Role::Schema && schema.fact.code.is_none())
        .map(|schema| schema.blob)
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
/// compiled, and Board's configuration and plugin manifests read, since
/// whether each is usable depends on its bytes alone.
fn blob_fact(classified: &Classified) -> BlobFact {
    let code = classified.code.or_else(|| match &classified.content {
        Content::Schema { value } => Schema::compile(value.clone())
            .err()
            .map(|error| error.code()),
        Content::Record { record_type, value } if record_type.as_str() == CONFIG_TYPE => {
            Config::from_value(value)
                .is_none()
                .then_some(Code::ConfigInvalid)
        }
        Content::Record { record_type, value } if record_type.as_str() == MANIFEST_TYPE => {
            Manifest::from_value(value.clone())
                .err()
                .map(|error| error.code())
        }
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
    source: &Source,
    path: &str,
    blob: ObjectId,
    role: Role,
) -> Option<Value> {
    if let Some(value) = values.get(&(blob, role)) {
        return Some(value.clone());
    }
    let bytes = repository.read(source, path, blob).ok()??;
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
            reason: error.reason(),
            failures: schema.failures(record) as u64,
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
    pub fn code(&self) -> Code {
        match self {
            Self::Git { source } => source.code(),
            Self::Index { source } => source.code(),
        }
    }
}

#[cfg(test)]
mod tests;

//! Refreshing the index from the working tree (docs/architecture/index.md).
//! Only files whose metadata changed are hashed, and every fact is keyed by
//! content, so a blob already indexed is not read again and a record is
//! validated again only against a schema blob it has not met.
//! A refresh that finds nothing changed writes nothing.

use std::collections::HashMap;

use board_index::{BlobFact, Index, IndexError, RefFact, Resolved, ValidationFact, View, Writer};
use board_model::{
    Classified, Code, Content, MAX_RECORD_BYTES, ObjectId, RecordType, Role, Source,
};
use board_tree::{Contents, Entry, GitError, Repository, latest};
use board_validate::Schema;
use serde_json::Value;

use crate::board::Board;
use crate::verdict::{Fault, against, judge};

/// The work a refresh did: files hashed, blobs parsed, schemas compiled and
/// records validated. Work-count tests assert these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Trees of `HEAD` listed.
    pub trees: usize,
    pub hashed: usize,
    pub parsed: usize,
    /// Schemas compiled, whether or not they compiled.
    pub compiled: usize,
    pub validated: usize,
    /// Whether it took the index's write lock.
    pub wrote: bool,
    /// Rows a prune deleted.
    pub pruned: usize,
}

/// What this refresh learnt from the blobs it read in full, so it need not
/// read or compile them again.
#[derive(Default)]
pub(crate) struct Kept {
    facts: HashMap<(ObjectId, Role), BlobFact>,
    /// Each record's parsed value.
    values: HashMap<ObjectId, Value>,
    /// Each schema compiled, or `None` when validation could not read it.
    /// Only a schema with no code is asked for, so it always compiles.
    schemas: HashMap<ObjectId, Option<Schema>>,
    compiled: usize,
}

/// One entry's blob, the role its path gives it, and what the index holds
/// about it.
#[derive(Clone, Debug)]
pub(crate) struct Indexed {
    pub(crate) blob: ObjectId,
    pub(crate) role: Role,
    pub(crate) fact: BlobFact,
    /// What checking a record against its schema found, when the index
    /// holds it.
    pub(crate) validation: Option<ValidationFact>,
}

/// What a refresh found, for a snapshot to decide each entry's status.
#[derive(Debug)]
pub(crate) struct Refresh {
    pub(crate) entries: Vec<Entry>,
    /// The fact for each entry that has one, by physical path. An entry
    /// that changed after the walk or could not be read has none.
    pub(crate) facts: HashMap<String, Indexed>,
    /// What `HEAD` holds at each path; empty before the first commit.
    pub(crate) head: HashMap<String, Option<ObjectId>>,
    pub(crate) work: Refreshed,
}

impl Board {
    /// Brings the index up to date with the working tree, adding every fact
    /// it does not yet hold.
    pub(crate) fn refresh(&self, writer: &Writer) -> Result<Refresh, RefreshError> {
        let mut work = Refreshed::default();
        let mut kept = Kept::default();
        let entries = self.walk(writer, &mut work, &mut kept)?;
        let facts = facts(
            writer,
            self.repository(),
            &Source::WorkingTree,
            &entries,
            &mut work,
            kept,
        )?;
        let head = self.head(writer, &mut work.trees)?;
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
        writer: &Writer,
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
    /// does not hold, and records what it saw. Each JSON file it reads in
    /// full is indexed into `kept` as it is read.
    fn walk(
        &self,
        writer: &Writer,
        work: &mut Refreshed,
        kept: &mut Kept,
    ) -> Result<Vec<Entry>, RefreshError> {
        let mut worktree = writer.worktree()?;
        let mut parsed = 0;
        let mut failed = None;
        let entries = self.repository().walk_working_tree(
            &mut |path, stat| worktree.known(path, stat),
            &mut |path, contents| {
                if failed.is_none()
                    && let Err(error) = keep(writer, kept, path, &contents, &mut parsed)
                {
                    failed = Some(error);
                }
            },
        )?;
        if let Some(error) = failed {
            return Err(error.into());
        }
        work.parsed += parsed;
        work.hashed += writer.sync_worktree(worktree, &entries)?;
        Ok(entries)
    }
}

/// What a run in a transaction returns: its value, and the entries to decide.
pub(crate) type Run<T> = Result<(T, Vec<Resolved>), RefreshError>;

/// Runs `run` in a transaction that takes the write lock only when it
/// writes, so that runs finding nothing changed do not wait on each other,
/// then decides the entries it returned. Returns what it returned, whether
/// it wrote, and the view of what it decided. If another process holds the
/// lock or wrote first, it runs again in a transaction that waits for the
/// lock, or in an index in memory if the wait runs out, as opening does.
/// The second run cannot be busy: it holds the write lock from its start,
/// and in WAL mode, which opening requires of a file, nothing else blocks it.
pub(crate) fn transact<T>(
    index: Index,
    mut run: impl FnMut(&Writer) -> Run<T>,
) -> Result<(T, bool, View), RefreshError> {
    let writer = index.deferred()?;
    let first = run(&writer);
    let (writer, result) = if busy(&first) {
        let writer = match writer.rollback()?.write() {
            Err(IndexError::Busy { .. }) => Index::fall_back()?.write()?,
            writer => writer?,
        };
        let second = run(&writer);
        (writer, second)
    } else {
        (writer, first)
    };
    let (value, entries) = result?;
    let wrote = writer.wrote()?;
    Ok((value, wrote, writer.decide(&entries)?))
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

/// Indexes the `contents` of the file at `path` that a walk read, unless
/// the index already holds its blob, and keeps what it holds.
fn keep(
    writer: &Writer,
    kept: &mut Kept,
    path: &str,
    contents: &Contents,
    parsed: &mut usize,
) -> Result<(), IndexError> {
    let Some(role) = Role::of(path) else {
        return Ok(());
    };
    let key = (contents.blob, role);
    if kept.facts.contains_key(&key) {
        return Ok(());
    }
    if let Some(fact) = writer.blob(contents.blob, role)? {
        kept.facts.insert(key, fact);
    } else {
        kept.index(writer, contents.blob, role, Some(&contents.bytes))?;
        *parsed += 1;
    }
    Ok(())
}

impl Kept {
    /// Classifies and judges the `bytes` of `blob` in `role`, or a blob too
    /// large to read, stores what the index keeps of it and keeps the rest.
    fn index(
        &mut self,
        writer: &Writer,
        blob: ObjectId,
        role: Role,
        bytes: Option<&[u8]>,
    ) -> Result<BlobFact, IndexError> {
        let classified = role.classify(bytes);
        let judged = self.judged(&classified);
        let fact = BlobFact {
            kind: classified.kind(),
            record_type: match &classified.content {
                Content::Record { record_type, .. } => Some(record_type.clone()),
                _ => None,
            },
            code: judged.as_ref().err().map(Fault::code),
        };
        writer.put_blob(blob, role, &fact)?;
        if let Content::Record { value, .. } = classified.content {
            writer.put_body(blob, &value)?;
            self.values.insert(blob, value);
        }
        // The same bytes in the reserved namespace are refused, and must not
        // hide the schema they compile to elsewhere.
        if let Ok(Some(schema)) = judged {
            self.schemas.insert(blob, Some(schema));
        }
        self.facts.insert((blob, role), fact.clone());
        Ok(fact)
    }

    /// Judges `classified`, counting each schema it compiles.
    fn judged(&mut self, classified: &Classified) -> Result<Option<Schema>, Fault> {
        let judged = judge(classified);
        if matches!(judged, Ok(Some(_)) | Err(Fault::Schema(_))) {
            self.compiled += 1;
        }
        judged
    }
}

/// What the index holds about each entry of `source` in `repository`,
/// parsing each blob and validating each record it does not hold yet. An
/// entry that changed after the walk is left out. What the walk `kept` is
/// not read again.
pub(crate) fn facts(
    writer: &Writer,
    repository: &Repository,
    source: &Source,
    entries: &[Entry],
    work: &mut Refreshed,
    mut kept: Kept,
) -> Result<HashMap<String, Indexed>, RefreshError> {
    let mut facts: HashMap<String, Indexed> = HashMap::new();
    for entry in entries {
        let (Some(blob), Some(role)) = (entry.blob, Role::of(&entry.path)) else {
            continue;
        };
        let fact = if let Some(fact) = kept.facts.get(&(blob, role)) {
            fact.clone()
        } else if let Some(fact) = writer.blob(blob, role)? {
            fact
        } else {
            // A blob that cannot be read has no content fact to store. Only a
            // blob the walk saw over the limit reads as too large.
            let (read, bytes) = match latest(repository.read(source, &entry.path, blob)) {
                Ok(Some(contents)) => (contents.blob, Some(contents.bytes)),
                Ok(None) if entry.size > Some(MAX_RECORD_BYTES as u64) => (blob, None),
                Ok(None) | Err(_) => continue,
            };
            let fact = kept.index(writer, read, role, bytes.as_deref())?;
            work.parsed += 1;
            // A file that changed since the walk is left out of this run,
            // which reports it unreadable.
            if read != blob {
                continue;
            }
            fact
        };
        let indexed = Indexed {
            blob,
            role,
            fact,
            validation: None,
        };
        facts.insert(entry.path.clone(), indexed);
    }
    let validations = validate_all(writer, repository, source, &facts, &mut kept, work)?;
    for (path, validation) in validations {
        if let Some(record) = facts.get_mut(&path) {
            record.validation = Some(validation);
        }
    }
    work.compiled += kept.compiled;
    Ok(facts)
}

/// What checking each record against its schema found, by path: from the
/// index when it holds that pair, or validated now and stored. A schema this
/// refresh did not compile is read and compiled once.
fn validate_all(
    writer: &Writer,
    repository: &Repository,
    source: &Source,
    facts: &HashMap<String, Indexed>,
    kept: &mut Kept,
    work: &mut Refreshed,
) -> Result<HashMap<String, ValidationFact>, RefreshError> {
    let mut found = HashMap::new();
    for (path, record) in facts {
        let Some(schema_path) = schema_type(record).map(RecordType::schema_path) else {
            continue;
        };
        let Some(schema) = schema_of(facts, &schema_path) else {
            continue;
        };
        if let Some(validation) = writer.validation(record.blob, schema)? {
            found.insert(path.clone(), validation);
            continue;
        }
        if !kept.schemas.contains_key(&schema) {
            let compiled = match repository.read(source, &schema_path, schema) {
                Ok(Some(contents)) => kept
                    .judged(&Role::Schema.classify(Some(&contents.bytes)))
                    .ok()
                    .flatten(),
                _ => None,
            };
            kept.schemas.insert(schema, compiled);
        }
        let Some(Some(compiled)) = kept.schemas.get(&schema) else {
            continue;
        };
        let Some(value) = known(&kept.values, repository, source, path, record.blob) else {
            continue;
        };
        let validation = validate(compiled, &value);
        writer.put_validation(record.blob, schema, &validation)?;
        work.validated += 1;
        found.insert(path.clone(), validation);
    }
    Ok(found)
}

/// The type of a record that is checked against its schema: one that
/// parsed, with a type outside the `board` namespace.
pub(crate) fn schema_type(record: &Indexed) -> Option<&RecordType> {
    let record_type = record.fact.record_type.as_ref()?;
    (record.fact.code.is_none() && !record_type.is_board()).then_some(record_type)
}

/// The blob of the usable schema at `path`.
pub(crate) fn schema_of(facts: &HashMap<String, Indexed>, path: &str) -> Option<ObjectId> {
    facts
        .get(path)
        .filter(|schema| schema.role == Role::Schema && schema.fact.code.is_none())
        .map(|schema| schema.blob)
}

/// The parsed value of a record, from this refresh or read again.
fn known(
    values: &HashMap<ObjectId, Value>,
    repository: &Repository,
    source: &Source,
    path: &str,
    blob: ObjectId,
) -> Option<Value> {
    if let Some(value) = values.get(&blob) {
        return Some(value.clone());
    }
    let contents = repository.read(source, path, blob).ok()??;
    match Role::Json.classify(Some(&contents.bytes)).content {
        Content::Record { value, .. } => Some(value),
        _ => None,
    }
}

fn validate(schema: &Schema, record: &Value) -> ValidationFact {
    match against(schema, record) {
        Ok(references) => ValidationFact::Valid(
            references
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

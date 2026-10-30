//! Refreshing the index from the working tree (docs/architecture/index.md).
//! Only files whose metadata changed are hashed, and every fact is keyed by
//! content, so a blob already indexed is not read again and a record is
//! validated again only against a schema blob it has not met.
//! A refresh that finds nothing changed writes nothing.

use std::collections::HashMap;

use board_index::{
    BlobFact, Decided, Index, IndexError, RefFact, Resolved, ValidationFact, Writer,
};
use board_model::{
    Canonical, Classified, Code, Content, MAX_RECORD_BYTES, ObjectId, RecordType, Role, Source,
    canonical_json, classify,
};
use board_tree::{Entry, GitError, Reader, Repository, latest};
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
    /// Working-tree files hashed. A file the last refresh could not open is
    /// opened again but not counted, even when it can now be read.
    pub hashed: usize,
    /// Blobs parsed. Each JSON or Markdown file hashed within the size limit is parsed
    /// on the thread that hashed it, so two files with the same bytes count
    /// twice though the blob is stored once; a blob read later counts once.
    pub parsed: usize,
    /// Schemas compiled, whether or not they compiled.
    pub compiled: usize,
    pub validated: usize,
    /// Whether it took the index's write lock.
    pub wrote: bool,
    /// Rows a prune deleted.
    pub pruned: usize,
    /// Files a snapshot's resolvers read to learn what they hold as a
    /// pointer. Pointers read while walking mounts are not counted.
    pub pointers_read: usize,
    /// Repositories a snapshot opened from disk, not counting the board's own.
    pub repositories: usize,
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
        let _timed = self.timed("index.refresh");
        let mut work = Refreshed::default();
        let mut kept = Kept::default();
        let entries = self.walk(writer, &mut work, &mut kept)?;
        let timed = self.timed("index.refresh.facts");
        let facts = facts(
            writer,
            self.repository(),
            &Source::WorkingTree,
            &entries,
            &mut work,
            kept,
        )?;
        drop(timed);
        let timed = self.timed("index.refresh.head");
        let head = self.head(writer, &mut work.trees)?;
        drop(timed);
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
    /// does not hold, and records what it saw. Each JSON file the walk reads
    /// in full is parsed on the thread that read it, and what the index does
    /// not hold yet is judged and stored into `kept` on this thread while
    /// the walk goes on, in the order the threads finish reading them; the
    /// index keys each fact by blob, so that order does not show.
    fn walk(
        &self,
        writer: &Writer,
        work: &mut Refreshed,
        kept: &mut Kept,
    ) -> Result<Vec<Entry>, RefreshError> {
        let timed = self.timed("index.refresh.load");
        let mut worktree = writer.worktree()?;
        drop(timed);
        let timed = self.timed("index.refresh.walk");
        let entries = self.repository().walk_working_tree(
            &mut |path, stat| worktree.known(path, stat),
            &|path, contents| {
                Role::of(path).map(|role| parse(contents.blob, role, Some(&contents.bytes)))
            },
            &mut |parsed| {
                if let Some(parsed) = parsed {
                    work.parsed += 1;
                    kept.keep(writer, parsed)?;
                }
                Ok::<(), RefreshError>(())
            },
        )?;
        drop(timed);
        let _timed = self.timed("index.refresh.sync");
        work.hashed += writer.sync_worktree(worktree, &entries)?;
        Ok(entries)
    }
}

/// What a run in a transaction returns: its value, and the entries to decide.
pub(crate) type Run<T> = Result<(T, Vec<Resolved>), RefreshError>;

/// Runs `run` in a transaction that takes the write lock only when it
/// writes, so that runs finding nothing changed do not wait on each other,
/// then commits. Returns what it returned, whether it wrote, and the index
/// decided, holding the entries it decided. If another process holds the lock or
/// wrote first, it runs again in a transaction that waits for the lock, or
/// in an index in memory if the wait runs out, as opening does. The second
/// run cannot be busy: it holds the write lock from its start, and in WAL
/// mode, which opening requires of a file, nothing else blocks it.
pub(crate) fn transact<T>(
    index: Index,
    run: impl FnMut(&Writer) -> Run<T>,
) -> Result<(T, bool, Decided), RefreshError> {
    let (value, wrote, writer, entries) = attempt(index, run)?;
    Ok((value, wrote, writer.decide(entries)?))
}

/// [`transact`] up to its commit: what `run` returned, whether it wrote, and
/// the writer and entries to commit.
pub(crate) fn attempt<T>(
    index: Index,
    mut run: impl FnMut(&Writer) -> Run<T>,
) -> Result<(T, bool, Writer, Vec<Resolved>), RefreshError> {
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
    Ok((value, wrote, writer, entries))
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

/// What a blob's bytes say about it before it is judged, parsed on any
/// thread: its classification, why it did not parse, and a record's
/// canonical body.
struct Parsed {
    blob: ObjectId,
    role: Role,
    classified: Classified,
    /// Why the bytes did not parse, which only the bytes can say.
    reason: Option<String>,
    body: Option<Canonical>,
    /// A Markdown file's links that name board paths, as written, outside
    /// its frontmatter.
    links: Vec<String>,
}

/// Classifies the `bytes` of `blob` in `role`, or a blob too large to
/// read. This reads nothing but `bytes`, so it runs on the thread that read
/// them; judging, which may compile a schema, waits until the index is
/// known not to hold the blob.
fn parse(blob: ObjectId, role: Role, bytes: Option<&[u8]>) -> Parsed {
    let classified = role.classify(bytes);
    let reason = classified
        .code
        .and_then(|code| Fault::Code(code).reason(role, bytes));
    let body = match &classified.content {
        Content::Record { value, .. } => Some(canonical_json(value)),
        _ => None,
    };
    let links = match (role, bytes) {
        (Role::Markdown, Some(bytes)) => board_model::links(bytes),
        _ => Vec::new(),
    };
    Parsed {
        blob,
        role,
        classified,
        reason,
        body,
        links,
    }
}

/// Judges `classified` by its bytes alone, and says whether that compiled
/// a schema, whether or not it compiled.
fn judged(classified: &Classified) -> (Result<Option<Schema>, Fault>, bool) {
    let judged = judge(classified);
    let compiled = matches!(judged, Ok(Some(_)) | Err(Fault::Schema(_)));
    (judged, compiled)
}

impl Kept {
    /// Keeps what `parsed` says: its fact from the index when that holds
    /// the blob in its role, else judged and stored now. A record's value is
    /// kept either way, so validating it does not read the file again.
    fn keep(&mut self, writer: &Writer, parsed: Parsed) -> Result<(), IndexError> {
        let key = (parsed.blob, parsed.role);
        if self.facts.contains_key(&key) {
            return Ok(());
        }
        let Some(fact) = writer.blob(parsed.blob, parsed.role)? else {
            self.store(writer, parsed)?;
            return Ok(());
        };
        self.facts.insert(key, fact);
        if let Content::Record { value, .. } = parsed.classified.content {
            self.values.insert(parsed.blob, value);
        }
        Ok(())
    }

    /// Classifies and judges the `bytes` of `blob` in `role`, or a blob too
    /// large to read, stores what the index keeps of it and keeps the rest.
    fn index(
        &mut self,
        writer: &Writer,
        blob: ObjectId,
        role: Role,
        bytes: Option<&[u8]>,
    ) -> Result<BlobFact, IndexError> {
        self.store(writer, parse(blob, role, bytes))
    }

    /// Judges `parsed`, stores what the index keeps of it and keeps the
    /// rest: a record's value, and a schema compiled.
    fn store(&mut self, writer: &Writer, parsed: Parsed) -> Result<BlobFact, IndexError> {
        let Parsed {
            blob,
            role,
            classified,
            reason,
            body,
            links,
        } = parsed;
        let (judged, compiled) = judged(&classified);
        self.compiled += usize::from(compiled);
        let fact = BlobFact {
            kind: classified.kind(),
            record_type: match &classified.content {
                Content::Record { record_type, .. } => Some(record_type.clone()),
                _ => None,
            },
            code: judged.as_ref().err().map(Fault::code),
            reason: match &judged {
                Ok(_) => None,
                Err(fault) => reason.or_else(|| fault.reason(role, None)),
            },
            links,
        };
        writer.put_blob(blob, role, &fact)?;
        if let Some(body) = &body {
            writer.put_body(blob, body)?;
        }
        if let Content::Record { value, .. } = classified.content {
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
    let reader = &mut repository.reader();
    // The index is keyed by blob, so blob order visits its keys in order.
    let mut entries: Vec<&Entry> = entries.iter().collect();
    entries.sort_unstable_by_key(|entry| entry.blob);
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
            let (read, bytes) = match latest(reader.read(source, &entry.path, blob)) {
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
    validate_all(writer, reader, source, &mut facts, &mut kept, work)?;
    work.compiled += kept.compiled;
    Ok(facts)
}

/// Sets what checking each record against its schema found: from the index
/// when it holds that pair, or validated now and stored. A schema this
/// refresh did not compile is read and compiled once.
fn validate_all(
    writer: &Writer,
    reader: &mut Reader<'_>,
    source: &Source,
    facts: &mut HashMap<String, Indexed>,
    kept: &mut Kept,
    work: &mut Refreshed,
) -> Result<(), RefreshError> {
    let mut schemas: HashMap<RecordType, Option<(String, ObjectId)>> = HashMap::new();
    for record_type in facts.values().filter_map(schema_type) {
        if !schemas.contains_key(record_type) {
            let path = record_type.schema_path();
            let schema = schema_of(facts, &path).map(|blob| (path, blob));
            schemas.insert(record_type.clone(), schema);
        }
    }
    let mut records: Vec<(&String, &mut Indexed)> = facts.iter_mut().collect();
    records.sort_unstable_by_key(|(_, record)| record.blob);
    for (path, record) in records {
        let Some((schema_path, schema)) = schema_type(record)
            .and_then(|record_type| schemas.get(record_type))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let schema = *schema;
        if let Some(validation) = writer.validation(record.blob, schema)? {
            record.validation = Some(validation);
            continue;
        }
        if !kept.schemas.contains_key(&schema) {
            let compiled = match reader.read(source, schema_path, schema) {
                Ok(Some(contents)) => {
                    let classified = Role::Schema.classify(Some(&contents.bytes));
                    let (judged, compiled) = judged(&classified);
                    kept.compiled += usize::from(compiled);
                    judged.ok().flatten()
                }
                _ => None,
            };
            kept.schemas.insert(schema, compiled);
        }
        let Some(Some(compiled)) = kept.schemas.get(&schema) else {
            continue;
        };
        let Some(value) = known(&kept.values, reader, source, path, record.blob) else {
            continue;
        };
        let validation = validate(compiled, &value);
        writer.put_validation(record.blob, schema, &validation)?;
        work.validated += 1;
        record.validation = Some(validation);
    }
    Ok(())
}

/// The type of a record that is checked against its schema: one that
/// parsed, with a type outside the `board` namespace.
pub(crate) fn schema_type(record: &Indexed) -> Option<&RecordType> {
    let record_type = record.fact.record_type.as_ref()?;
    (record.fact.code.is_none() && !record_type.is_reserved()).then_some(record_type)
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
    reader: &mut Reader<'_>,
    source: &Source,
    path: &str,
    blob: ObjectId,
) -> Option<Value> {
    if let Some(value) = values.get(&blob) {
        return Some(value.clone());
    }
    let contents = reader.read(source, path, blob).ok()??;
    match classify(path, Some(&contents.bytes)).content {
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
                | board_validate::RecordError::MalformedReference { pointer, .. } => pointer,
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

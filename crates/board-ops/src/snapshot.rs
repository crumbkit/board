//! One run's decision about every entry (docs/architecture/index.md). A
//! snapshot refreshes the index, walks the boards reachable through mounts,
//! indexing each mounted commit once however many places mount it, decides
//! every entry and commits. What depends only on content comes from the
//! index; whether a reference or pointer resolves depends on the whole tree,
//! so each run decides it again. The index's `entries` and `records` views
//! show what a snapshot decided, so they are read only through one. This
//! snapshot is a decided run over the index; `Repositories::snapshot` is a
//! different thing, a view of the git working tree for resolving paths.

use std::cell::RefCell;
use std::collections::HashMap;

use board_index::{Decided, Index, IndexError, Resolved, Storage, ValidationFact, View, Writer};
use board_model::{
    Change, Classified, Code, Content, Kind, Location, LogicalPath, Object, ObjectId, POINTER_TYPE,
    RecordType, Role, Source, Stat, classify,
};
use board_tree::{
    Cache, Entry, Mounted, Repositories, Repository, ResolveError, Status, TreeRow, TreeRows,
    in_scope, latest,
};

use crate::board::Board;
use crate::check::{CheckError, CheckReport, Checked, Detail, Failure, Tally, follow, unresolved};
use crate::index::{
    Indexed, Kept, RefreshError, Refreshed, Run, facts, schema_of, schema_type, transact,
};
use crate::verdict::{Fault, SchemaFound};

/// Every file each source's walk listed, sorted by path: the working tree
/// first, then each mounted commit.
pub(crate) type Walked = Vec<(Source, Vec<Entry>)>;

/// What one run decided about every entry it checked, and the index it
/// committed, whose views can show exactly that.
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) decided: Decided,
    pub(crate) report: CheckReport,
    /// Every reference each checked record makes, and where it led.
    pub(crate) references: Vec<Referenced>,
    pub(crate) walked: Walked,
}

impl Board {
    /// Decides every entry of the working tree and of each board it mounts,
    /// or only those at or beneath `scope`, after refreshing `index`. An
    /// entry's status is the first problem found: its path, then parsing,
    /// then its schema, validation and references, or for a pointer its
    /// target.
    pub(crate) fn snapshot(
        &self,
        index: Index,
        scope: Option<&LogicalPath>,
    ) -> Result<Snapshot, CheckError> {
        let _timed = self.timed("snapshot");
        if let Some(scope) = scope {
            let _timed = self.timed("resolve");
            self.repositories()
                .resolve(&Source::WorkingTree, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let ((mut report, references, walked), wrote, decided) =
            transact(index, |writer| self.decide(writer, scope))?;
        report.work.wrote = wrote;
        report.memory = match decided.storage() {
            Storage::Memory(fallback) => Some(*fallback),
            Storage::File(_) => None,
        };
        let work = &report.work;
        let mut trace = self.trace.borrow_mut();
        trace.count("entries", report.entries.len());
        trace.count("hashed", work.hashed);
        trace.count("parsed", work.parsed);
        trace.count("validated", work.validated);
        trace.count("trees", work.trees);
        drop(trace);
        Ok(Snapshot {
            decided,
            report,
            references,
            walked,
        })
    }

    /// Fills the index's views with what a snapshot decided, for a run that
    /// reads them: `query`, and `call` when the plugin extracts records.
    pub(crate) fn view(&self, decided: Decided) -> Result<View, IndexError> {
        let _timed = self.timed("view");
        decided.view()
    }

    /// Decides each entry, and returns the report, the references, the
    /// files each source's walk listed and the rows of `resolved`.
    fn decide(
        &self,
        writer: &Writer,
        scope: Option<&LogicalPath>,
    ) -> Run<(CheckReport, Vec<Referenced>, Walked)> {
        let refresh = self.refresh(writer)?;
        let mut work = refresh.work;
        let mounts = self.repositories().walk_mounts(
            &refresh.entries,
            &refresh.facts,
            &mut Indexing {
                writer,
                work: &mut work,
            },
        )?;
        let scopes = mounts.scopes(scope);
        let mut known = known(&refresh.entries, &mut work.hashed);
        let mut resolver = self.repositories().resolver(&mut known)?;
        let mut decided = Decisions::default();
        let root = Decide {
            repositories: self.repositories(),
            mounted: None,
            writer,
            listed: &refresh.entries,
            facts: &refresh.facts,
            resolve: RefCell::new(Box::new(|path: &LogicalPath| resolver.resolve(path))),
        };
        for entry in &refresh.entries {
            if !in_scope(&scopes, &Source::WorkingTree, entry.logical.as_ref()) {
                continue;
            }
            let change = if refresh.head.get(&entry.path) == Some(&entry.blob) {
                Change::Committed
            } else {
                Change::Uncommitted
            };
            decided.push(
                &root,
                &Source::WorkingTree,
                entry,
                entry.logical.clone(),
                change,
            )?;
        }
        drop(root);
        drop(resolver);
        drop(known);
        for mounted in &mounts.sources {
            let source = Source::Commit(mounted.commit);
            let decide = Decide {
                repositories: self.repositories(),
                mounted: Some(mounted),
                writer,
                listed: &mounted.entries,
                facts: &mounted.facts,
                resolve: RefCell::new(Box::new(|path: &LogicalPath| {
                    self.repositories()
                        .resolve_in(&mounted.repository, mounted.commit, path)
                })),
            };
            for entry in &mounted.entries {
                if !in_scope(&scopes, &source, entry.logical.as_ref()) {
                    continue;
                }
                let logical = entry
                    .logical
                    .as_ref()
                    .and_then(|logical| mounted.display(logical));
                decided.push(&decide, &source, entry, logical, Change::Committed)?;
            }
        }
        record(&mounts, writer, self.repository())?;
        // Every source is now stored, so a prune keeps what this run reached.
        if writer.wrote()? {
            work.pruned = writer.prune_if_doubled()?;
        }
        let report = CheckReport {
            entries: decided.checked,
            memory: None,
            work,
        };
        let mut walked = vec![(Source::WorkingTree, refresh.entries)];
        walked.extend(
            (mounts.sources.into_iter())
                .map(|mounted| (Source::Commit(mounted.commit), mounted.entries)),
        );
        Ok(((report, decided.references, walked), decided.resolved))
    }
}

/// The blob of each file `entries` saw as it is now, so it need not be
/// hashed again; each file it did not see as it is now counts in `hashed`.
fn known<'e>(
    entries: &'e [Entry],
    hashed: &'e mut usize,
) -> impl FnMut(&str, &Stat) -> Option<ObjectId> + 'e {
    let stats: HashMap<&str, (Stat, Option<ObjectId>)> = entries
        .iter()
        .filter_map(|entry| Some((entry.path.as_str(), (entry.stat?, entry.blob))))
        .collect();
    move |path, stat| {
        let seen = stats.get(path).filter(|(seen, _)| seen == stat);
        *hashed += usize::from(seen.is_none());
        seen.and_then(|(_, blob)| *blob)
    }
}

/// What a run decided: the report's entries and the rows of `resolved`,
/// built together so they cannot disagree.
#[derive(Default)]
struct Decisions {
    checked: Vec<Checked>,
    resolved: Vec<Resolved>,
    references: Vec<Referenced>,
}

/// One reference a record makes, and where it led this run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reached {
    pub(crate) pointer: String,
    pub(crate) target: LogicalPath,
    /// Where it resolved, or `None` when it dangles.
    pub(crate) location: Option<Location>,
    pub(crate) code: Option<Code>,
}

/// A reference, with the record that makes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Referenced {
    pub(crate) source: Source,
    pub(crate) path: String,
    pub(crate) logical: Option<LogicalPath>,
    pub(crate) reached: Reached,
}

impl Decisions {
    /// Decides `entry` of `source`, named `logical` from the root.
    fn push(
        &mut self,
        decide: &Decide<'_>,
        source: &Source,
        entry: &Entry,
        logical: Option<LogicalPath>,
        change: Change,
    ) -> Result<(), IndexError> {
        let fact = decide.facts.get(&entry.path);
        let role = Role::of(&entry.path);
        let kind = match (role, fact) {
            (None, _) => Kind::File,
            (Some(_), Some(fact)) => fact.fact.kind,
            (Some(_), None) => Kind::Json,
        };
        let wanted = fact.and_then(schema_type);
        let schema =
            wanted.map(|record_type| (record_type, decide.found(&record_type.schema_path())));
        let judged = match entry.status {
            Status::Degraded(code) => Ok(Some((code, None))),
            // Only a path too long under its mount point fails here.
            Status::Ok if logical.is_none() => Ok(Some((Code::PathMalformed, None))),
            Status::Ok => {
                let mut reached = Vec::new();
                let judged = decide.code(entry, &mut reached, role, fact, schema)?;
                self.references
                    .extend(reached.into_iter().map(|reached| Referenced {
                        source: source.clone(),
                        path: entry.path.clone(),
                        logical: logical.clone(),
                        reached,
                    }));
                judged
            }
        };
        let (failure, named) = match judged {
            Ok(failure) => (failure, None),
            Err(fault) => {
                let fields = fault.fields();
                let reason = match (&fields.pointer, &fault) {
                    (Some(pointer), _) => format!("names a type at {pointer:?}: {fault}"),
                    (
                        None,
                        Fault::NoSchema {
                            found: SchemaFound::Missing,
                            ..
                        },
                    ) => format!("names an unknown type: {fault}"),
                    (None, _) => format!("cannot be checked: {fault}"),
                };
                let detail = Detail {
                    pointer: fields.pointer,
                    reason,
                    failures: 1,
                    ..Detail::default()
                };
                (Some((fault.code(), Some(detail))), fields.schema)
            }
        };
        let (code, detail) = failure.map_or((None, None), |(code, detail)| (Some(code), detail));
        // The file at fault is the entry itself unless the detail names
        // another, which says its own mount.
        let detail = detail.map(|detail| match detail.at {
            None => Detail {
                mount: decide.route(),
                ..detail
            },
            Some(_) => detail,
        });
        self.resolved.push(Resolved {
            source: source.clone(),
            path: entry.path.clone(),
            logical: logical.clone(),
            blob: entry.blob,
            size: entry.size,
            role,
            schema: schema.and_then(|(_, found)| found.ok()),
            kind,
            code,
            change,
        });
        self.checked.push(Checked {
            source: source.clone(),
            path: entry.path.clone(),
            logical,
            kind,
            status: code.map_or(Status::Ok, Status::Degraded),
            detail,
            change,
            schema: named,
        });
        Ok(())
    }
}

/// An entry's failure, or the fault of a record with no usable schema.
type Judged = Result<Option<Failure>, Fault>;

/// A record's type, and the blob of the usable schema at the path it names
/// from its source's root, or what is there instead.
type Wanted<'a> = Option<(&'a RecordType, Result<ObjectId, SchemaFound>)>;

/// Resolves a path in the source being checked.
type Resolve<'a> = Box<dyn FnMut(&LogicalPath) -> Result<Location, ResolveError> + 'a>;

/// Why an entry the walk listed has no fact.
const READ_AGAIN: &str = "changed or could not be read; run the command again";

/// What deciding the status of a source's entries reads.
struct Decide<'a> {
    repositories: &'a Repositories,
    /// The mounted source being decided; `None` for the root's.
    mounted: Option<&'a Mounted<Facts>>,
    writer: &'a Writer,
    /// Every entry the walk listed, with a fact or not, sorted by path.
    listed: &'a [Entry],
    /// What the index holds about the source's entries, by physical path.
    facts: &'a HashMap<String, Indexed>,
    resolve: RefCell<Resolve<'a>>,
}

impl Decide<'_> {
    /// The failure of an entry whose path is fine.
    fn code(
        &self,
        entry: &Entry,
        reached: &mut Vec<Reached>,
        role: Option<Role>,
        fact: Option<&Indexed>,
        schema: Wanted<'_>,
    ) -> Result<Judged, IndexError> {
        if role.is_none() {
            return Ok(Ok(None));
        }
        // A file that changed after the walk, was removed or could not be
        // read has no fact.
        let Some(fact) = fact else {
            let detail = Detail {
                reason: READ_AGAIN.to_owned(),
                failures: 1,
                ..Detail::default()
            };
            return Ok(Ok(Some((Code::RepoUnreadable, Some(detail)))));
        };
        if let Some(code) = fact.fact.code {
            let detail = fact.fact.reason.clone().map(|reason| Detail {
                reason,
                failures: 1,
                ..Detail::default()
            });
            return Ok(Ok(Some((code, detail))));
        }
        match fact.fact.kind {
            Kind::Pointer => Ok(Ok(entry
                .logical
                .as_ref()
                .and_then(|logical| self.resolve(logical).err())
                .map(|error| {
                    let (code, detail) = unresolved(&error);
                    let mount = detail.at.as_ref().and_then(|at| self.mount(at));
                    let at = detail.at.as_ref().and_then(|at| self.display(at));
                    (
                        code,
                        Some(Detail {
                            at,
                            mount,
                            ..detail
                        }),
                    )
                }))),
            Kind::Record => self.record(fact, schema, reached),
            Kind::Schema | Kind::Json | Kind::File => Ok(Ok(None)),
        }
    }

    /// Checks a record against its schema and its references.
    fn record(
        &self,
        record: &Indexed,
        wanted: Wanted<'_>,
        reached: &mut Vec<Reached>,
    ) -> Result<Judged, IndexError> {
        let Some((record_type, schema)) = wanted else {
            return Ok(Ok(None));
        };
        if let Err(found) = schema {
            return Ok(Err(Fault::NoSchema {
                record_type: record_type.clone(),
                found,
            }));
        }
        let references = match record.validation.clone() {
            None => {
                let detail = Detail {
                    pointer: None,
                    reason: format!("cannot be checked: it or its schema {READ_AGAIN}"),
                    failures: 1,
                    at: None,
                    mount: None,
                };
                return Ok(Ok(Some((Code::RepoUnreadable, Some(detail)))));
            }
            Some(ValidationFact::Invalid {
                code,
                pointer,
                reason,
                failures,
            }) => {
                let pointer = Some(pointer);
                let detail = Detail {
                    pointer,
                    reason,
                    failures,
                    ..Detail::default()
                };
                return Ok(Ok(Some((code, Some(detail)))));
            }
            Some(ValidationFact::Valid(references)) => references,
        };
        // Every reference is followed, not just those before the first
        // failure, so `refs` can report each one.
        let mut tally = Tally::default();
        for reference in references {
            let (location, failure) = follow(
                &reference.target,
                reference.accepts.as_deref(),
                self.resolve(&reference.target),
                |location| self.record_type(location),
            )?;
            tally.add(&reference.pointer, failure.as_ref());
            reached.push(Reached {
                pointer: reference.pointer,
                target: reference.target,
                location,
                code: failure.map(|(code, _)| code),
            });
        }
        Ok(Ok(tally.first().map(|(code, detail)| (code, Some(detail)))))
    }

    /// The blob of the usable schema at `path`, or what is there instead:
    /// a file the walk listed with no fact, or a degraded directory above
    /// it, could not be read.
    fn found(&self, path: &str) -> Result<ObjectId, SchemaFound> {
        schema_of(self.facts, path).ok_or_else(|| {
            let above = path.rmatch_indices('/').map(|(at, _)| &path[..at]);
            if self.facts.contains_key(path) {
                SchemaFound::Unusable
            } else if self.listed(path).is_some()
                || above
                    .filter_map(|directory| self.listed(directory))
                    .any(|entry| entry.status != Status::Ok)
            {
                SchemaFound::Unreadable
            } else {
                SchemaFound::Missing
            }
        })
    }

    /// The entry the walk listed at `path`.
    fn listed(&self, path: &str) -> Option<&Entry> {
        let at = self
            .listed
            .binary_search_by(|entry| entry.path.as_str().cmp(path));
        at.ok().map(|at| &self.listed[at])
    }

    fn resolve(&self, path: &LogicalPath) -> Result<Location, ResolveError> {
        (self.resolve.borrow_mut())(path)
    }

    /// The mount point of the source, from the root; `None` for the root.
    fn route(&self) -> Option<LogicalPath> {
        self.mounted?.route.parse().ok()
    }

    /// `path`, a path in the source, from the root.
    fn display(&self, path: &LogicalPath) -> Option<LogicalPath> {
        match self.mounted {
            Some(mounted) => mounted.display(path),
            None => Some(path.clone()),
        }
    }

    /// The mount point, from the root, of the mounted board that holds
    /// `at`, a path in the source: the board the directory above `at`
    /// resolves into. `None` when that is the root board.
    fn mount(&self, at: &LogicalPath) -> Option<LogicalPath> {
        let parent = match at.as_str().rsplit_once('/') {
            Some(("", _)) | None => "/",
            Some((parent, _)) => parent,
        };
        let location = self.resolve(&parent.parse().ok()?).ok()?;
        if location.board.source == Source::WorkingTree {
            return None;
        }
        let root = Location {
            path: String::new(),
            ..location
        };
        self.display(&root.display_path()?)
    }

    /// The `$type` of the record at `location`: from the index when it
    /// holds the blob, which is a fact about content wherever it is, or read
    /// in its own board.
    fn record_type(&self, location: &Location) -> Result<Option<RecordType>, IndexError> {
        let Object::Blob(blob) = location.object else {
            return Ok(None);
        };
        let Some(role) = Role::of(&location.path) else {
            return Ok(None);
        };
        let Some(fact) = self.writer.blob(blob, role)? else {
            return Ok(read_type(self.repositories, location, blob));
        };
        Ok(match (fact.code, fact.kind) {
            (Some(_), _) => None,
            (None, Kind::Pointer) => POINTER_TYPE.parse().ok(),
            (None, _) => fact.record_type,
        })
    }
}

/// The `$type` of the record at `location`, read in its own board.
pub(crate) fn read_type(
    repositories: &Repositories,
    location: &Location,
    blob: ObjectId,
) -> Option<RecordType> {
    let contents = latest(repositories.read(location, blob)).ok()??;
    match classify(&location.path, Some(&contents.bytes)) {
        Classified {
            content: Content::Record { record_type, .. },
            code: None,
        } => Some(record_type),
        Classified {
            content: Content::Pointer { .. },
            code: None,
        } => POINTER_TYPE.parse().ok(),
        _ => None,
    }
}

/// What the index holds about one source's entries, by physical path.
type Facts = HashMap<String, Indexed>;

/// Every mounted source, with what the index holds about its entries.
type Mounts = board_tree::Reached<Facts>;

/// The index as the cache a walk through mounts reads and fills.
struct Indexing<'w> {
    writer: &'w Writer,
    work: &'w mut Refreshed,
}

// A cache read that fails counts as a miss: the index is disposable.
impl Cache for Indexing<'_> {
    type Error = RefreshError;
    type Facts = Facts;

    fn has_tree(&mut self, tree: ObjectId) -> bool {
        self.writer.has_tree(tree).unwrap_or(false)
    }

    fn put_tree(&mut self, tree: &TreeRows) -> Result<(), RefreshError> {
        self.writer.put_tree(tree)?;
        self.work.trees += 1;
        Ok(())
    }

    fn rows(&mut self, tree: ObjectId) -> Option<Vec<TreeRow>> {
        self.writer.tree(tree).ok().flatten()
    }

    fn facts(
        &mut self,
        repository: &Repository,
        source: &Source,
        entries: &[Entry],
    ) -> Result<Facts, RefreshError> {
        let kept = Kept::default();
        facts(self.writer, repository, source, entries, self.work, kept)
    }

    fn is_pointer(facts: &Facts, path: &str) -> bool {
        facts.get(path).is_some_and(|indexed| {
            indexed.fact.kind == Kind::Pointer && indexed.fact.code.is_none()
        })
    }
}

/// Stores every source, the root's repository first, and every mount.
fn record(mounts: &Mounts, writer: &Writer, root: &Repository) -> Result<(), IndexError> {
    let mut sources = vec![(
        Source::WorkingTree,
        root.head_tree().ok().flatten(),
        root.path(),
    )];
    sources.extend(mounts.sources.iter().map(|mounted| {
        (
            Source::Commit(mounted.commit),
            Some(mounted.root),
            mounted.repository.path(),
        )
    }));
    writer.set_mounts(&sources, &mounts.points)
}

#[cfg(test)]
mod tests;

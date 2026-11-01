//! Deciding each entry of one source: its path, then parsing, then its
//! schema, validation and references, or for a pointer its target.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::convert::Infallible;

use board_index::{IndexError, Resolved, ValidationFact, Writer};
use board_model::{
    BINDING_TYPE, Board, Change, Classified, Code, Content, Kind, Location, LogicalPath, Object,
    ObjectId, REPO_TYPE, RecordType, Repo, Role, Source, classify, parse_json,
};
use board_tree::{
    Contents, Entry, GitError, Mounted, Reader, Refused, Repositories, ResolveError, Status, latest,
};

use super::Facts;
use crate::check::{Checked, Detail, Failure, Tally, follow, unresolved};
use crate::index::{Indexed, schema_of, schema_type};
use crate::ui::fitted;
use crate::verdict::{Fault, SchemaFound};

/// Why the symlink `entry` leads to no file, when `code` says it does not.
fn symlink(entry: &Entry, code: Code) -> Option<Detail> {
    let link = entry.link.as_ref()?;
    matches!(
        code,
        Code::SymlinkOutside | Code::SymlinkLoop | Code::SymlinkDangling
    )
    .then(|| Detail {
        reason: format!(
            "is {}",
            Refused {
                code,
                target: link.target.clone()
            }
        ),
        failures: 1,
        target: Some(link.target.clone()),
        ..Detail::default()
    })
}

/// What a run decided: the report's entries and the rows of `resolved`,
/// built together so they cannot disagree.
#[derive(Default)]
pub(super) struct Decisions {
    pub(super) checked: Vec<Checked>,
    pub(super) resolved: Vec<Resolved>,
    pub(super) references: Vec<Referenced>,
}

/// One reference a record makes, and where it led this run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reached {
    pub(crate) pointer: String,
    /// Whether it is a Markdown link, whose `pointer` is its destination.
    pub(crate) link: bool,
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
    pub(super) fn push(
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
            (None, _) | (Some(Role::Markdown), None) => Kind::File,
            (Some(_), Some(fact)) => fact.fact.kind,
            (Some(_), None) => Kind::Json,
        };
        let wanted = fact.and_then(schema_type);
        let schema =
            wanted.map(|record_type| (record_type, decide.found(&record_type.schema_path())));
        let judged = match entry.status {
            Status::Degraded(code) => Ok(Some((code, symlink(entry, code)))),
            // Only a path too long under its mount point fails here.
            Status::Ok if logical.is_none() => Ok(Some((Code::PathMalformed, None))),
            Status::Ok => {
                let mut reached = Vec::new();
                let judged = decide.code(entry, &mut reached, role, fact, schema.clone())?;
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
                    target: fields.target,
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
pub(super) struct Decide<'a> {
    pub(super) repositories: &'a Repositories,
    /// The mounted source being decided; `None` for the root's.
    pub(super) mounted: Option<&'a Mounted<Facts>>,
    pub(super) writer: &'a Writer,
    /// Every entry the walk listed, with a fact or not, sorted by path.
    pub(super) listed: &'a [Entry],
    /// What the index holds about the source's entries, by physical path.
    pub(super) facts: &'a HashMap<String, Indexed>,
    pub(super) resolve: RefCell<Resolve<'a>>,
    /// The root repository's reader, for a reference into the working tree.
    pub(super) reader: RefCell<Reader<'a>>,
    /// Whether a read or resolution failed for a reason that may pass, so
    /// that what was decided must not be kept.
    pub(super) failed: Cell<bool>,
    /// The `remote` of each valid repo record of the source, by physical
    /// path, with its path from the root; read once, when first needed.
    pub(super) remotes: OnceCell<HashMap<String, (String, String)>>,
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
        if let Some(failure) = self.duplicate(entry, fact) {
            return Ok(Ok(Some(failure)));
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
            // A Markdown record's links are followed after its references,
            // so its first failure is in its frontmatter; a JSON record has
            // no links.
            Kind::Record => {
                let judged = self.record(fact, schema, reached)?;
                let links = self.links(entry, &fact.fact.links, reached);
                Ok(judged.map(|failure| failure.or_else(|| self.fitted(entry, fact)).or(links)))
            }
            Kind::File => Ok(Ok(self.links(entry, &fact.fact.links, reached))),
            Kind::Schema | Kind::Json => Ok(Ok(None)),
        }
    }

    /// The failure of a repo record whose `remote` another repo record of
    /// the source holds too.
    fn duplicate(&self, entry: &Entry, fact: &Indexed) -> Option<Failure> {
        (fact.fact.record_type.as_ref()?.as_str() == REPO_TYPE).then_some(())?;
        let remotes = self.remotes.get_or_init(|| self.read_remotes());
        let (remote, _) = remotes.get(&entry.path)?;
        let mut others: Vec<&str> = (remotes.iter())
            .filter(|(path, (other, _))| other == remote && **path != entry.path)
            .map(|(_, (_, shown))| shown.as_str())
            .collect();
        if others.is_empty() {
            return None;
        }
        others.sort_unstable();
        let detail = Detail {
            reason: format!(
                "names the repository {remote}, which {} also names",
                others.join(", ")
            ),
            failures: 1,
            ..Detail::default()
        };
        Some((Code::RepoDuplicateRemote, Some(detail)))
    }

    /// The `remote` of each valid repo record the walk listed, by physical
    /// path, with its path from the root.
    fn read_remotes(&self) -> HashMap<String, (String, String)> {
        let board = match self.mounted {
            Some(mounted) => Board {
                repository: mounted.repository.path().to_path_buf(),
                source: Source::Commit(mounted.commit),
            },
            None => Board {
                repository: self.repositories.root().path().to_path_buf(),
                source: Source::WorkingTree,
            },
        };
        let repo = |entry: &Entry| {
            let indexed = self.facts.get(&entry.path)?;
            let fact = &indexed.fact;
            let is_repo = fact.record_type.as_ref().map(RecordType::as_str) == Some(REPO_TYPE);
            (is_repo && fact.code.is_none() && entry.status == Status::Ok).then_some(())?;
            let location = Location {
                board: board.clone(),
                path: entry.path.clone(),
                object: Object::Blob(indexed.blob),
                pointers: Vec::new(),
            };
            let contents = self.read(&location, indexed.blob).ok()??;
            let remote = Repo::from_value(&parse_json(&contents.bytes).ok()?)
                .ok()?
                .remote;
            let shown = self.display(entry.logical.as_ref()?)?.to_string();
            Some((entry.path.clone(), (remote, shown)))
        };
        self.listed.iter().filter_map(repo).collect()
    }

    /// Follows the links of a Markdown file, each from the file's own path,
    /// as a record's references are followed. A link to nothing degrades
    /// the file as a dangling reference degrades a record. To report it in
    /// `refs` alone, return `None` here and give a Markdown file no
    /// references in put/file.rs.
    fn links(
        &self,
        entry: &Entry,
        links: &[String],
        reached: &mut Vec<Reached>,
    ) -> Option<Failure> {
        let file = entry.logical.as_ref()?;
        let mut tally = Tally::default();
        for link in links {
            let Some(target) = board_model::reached(file, link) else {
                continue;
            };
            let Ok((location, failure)) =
                follow::<Infallible>(&target, None, self.resolve(&target), |_| Ok(None));
            tally.add(link, failure.as_ref());
            reached.push(Reached {
                pointer: link.clone(),
                link: true,
                target,
                location,
                code: failure.map(|(code, _)| code),
            });
        }
        tally.first().map(|(code, detail)| (code, Some(detail)))
    }

    /// Checks a record against its schema and its references.
    pub(super) fn record(
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
                    target: None,
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
                link: false,
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
            let link = self.listed(path).and_then(|entry| match entry.status {
                Status::Degraded(
                    code @ (Code::SymlinkOutside | Code::SymlinkLoop | Code::SymlinkDangling),
                ) => Some((code, entry.link.as_ref()?)),
                _ => None,
            });
            if self.facts.contains_key(path) {
                SchemaFound::Unusable
            } else if let Some((code, link)) = link {
                SchemaFound::Symlink(Refused {
                    code,
                    target: link.target.clone(),
                })
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
        let resolved = (self.resolve.borrow_mut())(path);
        if let Err(ResolveError::Git(_) | ResolveError::Unreadable { .. }) = resolved {
            self.failed.set(true);
        }
        resolved
    }

    /// The bytes of `blob` at `location`, read in its own board.
    fn read(&self, location: &Location, blob: ObjectId) -> Result<Option<Contents>, GitError> {
        let root = &mut self.reader.borrow_mut();
        let read = latest(self.repositories.read(root, location, blob));
        self.failed.set(self.failed.get() || read.is_err());
        read
    }

    /// The mount point of the source, from the root; `None` for the root.
    fn route(&self) -> Option<LogicalPath> {
        Some(self.mounted?.route.clone())
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
        let location = self.resolve(&at.parent()).ok()?;
        if location.board.source == Source::WorkingTree {
            return None;
        }
        let root = Location {
            path: String::new(),
            ..location
        };
        self.display(&root.display_path()?)
    }

    /// The `$type` of the record at `location`: from the facts the refresh
    /// loaded when the entry at its path holds the same blob, else from the
    /// index when it holds the blob, or read in its own board. A fact is
    /// about content, so it holds wherever the blob is.
    pub(super) fn record_type(
        &self,
        location: &Location,
    ) -> Result<Option<RecordType>, IndexError> {
        let Object::Blob(blob) = location.object else {
            return Ok(None);
        };
        let Some(role) = Role::of(&location.path) else {
            return Ok(None);
        };
        let own = (self.facts.get(&location.path)).filter(|indexed| indexed.blob == blob);
        let fact = match own {
            Some(indexed) => indexed.fact.clone(),
            None => match self.writer.blob(blob, role)? {
                Some(fact) => fact,
                None => return Ok(self.read_type(location, blob)),
            },
        };
        Ok(match fact.code {
            Some(_) => None,
            None => fact.record_type,
        })
    }

    fn read_type(&self, location: &Location, blob: ObjectId) -> Option<RecordType> {
        type_of(&location.path, &self.read(location, blob).ok()??)
    }

    /// The first way `entry`, when it is a binding, does not fit its view,
    /// read in the source being checked.
    fn fitted(&self, entry: &Entry, record: &Indexed) -> Option<Failure> {
        (record.fact.record_type.as_ref()?.as_str() == BINDING_TYPE).then_some(())?;
        let path = entry.logical.as_ref()?;
        let resolve = &mut |path: &LogicalPath| self.resolve(path);
        let read = &mut |location: &Location, blob| self.read(location, blob);
        let fitted = fitted((path, None), read, resolve);
        fitted.map(|(code, detail)| (code, Some(detail)))
    }
}

/// The `$type` of the record at `location`, read in its own board.
pub(crate) fn read_type(
    repositories: &Repositories,
    root: &mut Reader<'_>,
    location: &Location,
    blob: ObjectId,
) -> Option<RecordType> {
    let contents = latest(repositories.read(root, location, blob)).ok()??;
    type_of(&location.path, &contents)
}

/// The `$type` of the record `contents` hold at `path`.
fn type_of(path: &str, contents: &Contents) -> Option<RecordType> {
    match classify(path, Some(&contents.bytes)) {
        Classified {
            content: Content::Record { record_type, .. } | Content::Pointer { record_type, .. },
            code: None,
        } => Some(record_type),
        _ => None,
    }
}

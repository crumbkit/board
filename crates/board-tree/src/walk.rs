//! Walking every entry of a source without following pointers
//! (docs/architecture.md, Boards and entries).

use std::fmt;

use board_model::{Code, LogicalPath, ObjectId, Source, Stat};

use crate::address::{self, Pointers};
use crate::git::{GitError, Repository};
use crate::tree::{Kind, RawEntry, Tree, Unhashed};
use crate::worktree::{Kept, Known};

/// One file in a tree. Directories are not entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path inside the tree, `/`-separated; a name that is not UTF-8 is
    /// shown lossily.
    pub path: String,
    /// The address, unless the path or one of its directories is malformed.
    pub logical: Option<LogicalPath>,
    /// The blob; `None` for a submodule or a file or directory that cannot be
    /// read, since derived data is keyed by blob alone.
    pub blob: Option<ObjectId>,
    pub status: Status,
    /// A working-tree file's or symlink's metadata.
    pub stat: Option<Stat>,
    /// The blob's size in bytes, when known.
    pub size: Option<u64>,
}

/// One name inside a directory: the last segment of its logical path, and
/// whether a directory is there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub segment: String,
    pub directory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded(Code),
}

/// A directory still to be listed, with the code it passes to everything
/// beneath it.
struct Pending<D> {
    directory: D,
    path: String,
    logical: Option<LogicalPath>,
    inherited: Option<Code>,
}

impl Repository {
    /// Every file in `source`, sorted by path. An entry's status is the first
    /// problem on its path, its directories' included. Only a root that
    /// cannot be read is an error: anything beneath it that cannot be read is
    /// one degraded entry, and the walk goes on.
    pub fn walk(&self, source: &Source) -> Result<Vec<Entry>, GitError> {
        self.walk_under(source, "")
    }

    /// The files beneath the directory at the `/`-separated physical `path`
    /// in `source`, as [`Repository::walk`] reports them, reading only that
    /// directory's subtree and the directories on the way down to it. An
    /// empty path is the root.
    pub fn walk_under(&self, source: &Source, path: &str) -> Result<Vec<Entry>, GitError> {
        match source {
            Source::Commit(commit) => walk_from(&mut self.commit_tree(*commit)?, path),
            Source::WorkingTree => walk_from(&mut self.working_tree(None)?, path),
        }
    }

    /// The names one level inside the directory at the `/`-separated
    /// physical `path` in `source`, each with a logical path, without
    /// following pointers. No file is hashed, and only a record beside its
    /// directory is read, so the cost is one directory listing; nothing is
    /// there when no directory is.
    pub fn names(&self, source: &Source, path: &str) -> Result<Vec<Name>, GitError> {
        match source {
            Source::Commit(commit) => names_in(&mut self.commit_tree(*commit)?, path),
            // A name needs no blob, so every file is given the null id
            // instead of being hashed.
            Source::WorkingTree => names_in(
                &mut self.working_tree(Some(&mut |_, _| Some(ObjectId::Sha1([0; 20]))))?,
                path,
            ),
        }
    }

    /// Walks the working tree as [`Repository::walk`] does, but hashes a
    /// file only when `known`, given its `/`-separated path and metadata,
    /// does not already know its blob. The directories are listed on the
    /// repository's threads ([`Repository::set_threads`]), and the files
    /// are hashed on as many once every directory is listed. Each
    /// JSON file hashed within
    /// [`MAX_RECORD_BYTES`](board_model::MAX_RECORD_BYTES) is read in full,
    /// and `kept` takes its path and contents as soon as it is read, on the
    /// thread that read it, so that the caller holds no more of the tree
    /// than it chooses to. The entries are the same however many threads
    /// hash; only the order `kept` sees differs.
    pub fn walk_working_tree(
        &self,
        known: Known<'_>,
        kept: Kept<'_>,
    ) -> Result<Vec<Entry>, GitError> {
        walk_in(&mut self.working_tree(Some(known))?.keeping(kept).on_demand())
    }
}

pub(crate) fn walk_in<T: Tree>(tree: &mut T) -> Result<Vec<Entry>, GitError> {
    walk_from(tree, "")
}

/// How far a walk reaches down one path.
struct Descent<'p, D> {
    /// The deepest directory on the path that exists.
    at: Pending<D>,
    /// The directory on the way whose own code `at` inherits.
    fault: Option<Beneath>,
    /// The path's segments past `at`.
    rest: Vec<&'p str>,
}

/// The code a walk would give a file once it is written, and whose code it
/// is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Planned {
    /// The file's own code.
    File(Code),
    /// The code of a directory the file would be in.
    Directory(Beneath),
}

/// A directory with its own code, which a file written beneath it would
/// inherit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beneath {
    pub code: Code,
    /// Its path from the board's root, without a leading `/`.
    pub directory: String,
    /// Whether the write would create it.
    pub created: bool,
    /// The existing entry beside it whose name differs only in case.
    pub beside: Option<String>,
}

impl Beneath {
    /// The directory `name` with its own `code`, among the `alike` entries
    /// of its parent, at `parent`.
    fn new<D>(code: Code, parent: &str, alike: &[RawEntry<D>], name: &str, created: bool) -> Self {
        let beside = alike
            .iter()
            .find(|entry| {
                entry.name != name.as_bytes() && entry.name.eq_ignore_ascii_case(name.as_bytes())
            })
            .filter(|_| code == Code::PathCaseCollision)
            .map(|entry| join(parent, &String::from_utf8_lossy(&entry.name)));
        Self {
            code,
            directory: join(parent, name),
            created,
            beside,
        }
    }
}

impl fmt::Display for Beneath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            code,
            directory,
            created,
            beside,
        } = self;
        let (place, tense) = if *created {
            ("it would create", Tense::Would)
        } else {
            ("above it", Tense::Is)
        };
        let reason = reason(*code, tense, beside.as_deref());
        write!(formatter, "the directory {directory} {place} {reason}")
    }
}

/// Whether an entry with a code exists, or would once a write makes it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Tense {
    Is,
    Would,
}

/// What is wrong with an entry that a walk gives `code`, said of it in
/// `tense`. `other` names the entry beside it that it collides with, when
/// that is known.
pub(crate) fn reason(code: Code, tense: Tense, other: Option<&str>) -> String {
    let other = other.unwrap_or("another entry beside it");
    match (code, tense) {
        (Code::PathShadowed, Tense::Is) => format!("shares its name with {other}"),
        (Code::PathShadowed, Tense::Would) => format!("would share its name with {other}"),
        (Code::PathCaseCollision, Tense::Is) => format!("differs only in case from {other}"),
        (Code::PathCaseCollision, Tense::Would) => {
            format!("would differ only in case from {other}")
        }
        (Code::PathMalformed, Tense::Is) => "has no valid logical path".to_owned(),
        (Code::PathMalformed, Tense::Would) => "would have no valid logical path".to_owned(),
        (Code::RepoUnreadable, Tense::Is) => {
            "cannot be read, or an entry that may share its name cannot be read".to_owned()
        }
        (Code::RepoUnreadable, Tense::Would) => {
            "would be beside an entry that may share its name and cannot be read".to_owned()
        }
        (Code::RepoSubmodule, Tense::Is) => "is a submodule".to_owned(),
        // The walk gives a new file only shadowed, case_collision, malformed
        // or unreadable, and an existing directory those or submodule; this
        // covers any code it gains later.
        (_, Tense::Is) => "is degraded".to_owned(),
        (_, Tense::Would) => "would be degraded".to_owned(),
    }
}

/// The deepest directory on the `/`-separated `path`, reached from the root
/// as a walk reaches it: with its logical path and the code its ancestors
/// pass down. Only the entries alike to each segment are addressed, since no
/// others bear on its code.
fn descend<'p, T: Tree>(
    tree: &mut T,
    path: &'p str,
) -> Result<Descent<'p, T::Directory>, GitError> {
    let mut at = Pending {
        directory: tree.root(),
        path: String::new(),
        logical: Some(LogicalPath::root()),
        inherited: None,
    };
    let mut fault = None;
    let mut segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .peekable();
    while let Some(&segment) = segments.peek() {
        let alike = tree.alike(&at.directory, segment.as_bytes(), Some(segment))?;
        let addressed = match &at.logical {
            Some(parent) => tree.address(&at.directory, parent, &alike),
            None => vec![(None, None); alike.len()],
        };
        let found = alike
            .iter()
            .zip(addressed)
            .find(|(entry, _)| entry.name == segment.as_bytes());
        let Some((Kind::Directory(child), (logical, own))) =
            found.map(|(entry, addressed)| (&entry.kind, addressed))
        else {
            break;
        };
        if let (None, Some(code)) = (&fault, own) {
            fault = Some(Beneath::new(code, &at.path, &alike, segment, false));
        }
        at = Pending {
            directory: child.clone(),
            path: join(&at.path, segment),
            logical,
            inherited: at.inherited.or(own),
        };
        segments.next();
    }
    Ok(Descent {
        at,
        fault,
        rest: segments.collect(),
    })
}

/// The code a walk would give the `/`-separated `file` once it is written,
/// holding a pointer when `is_pointer`, and whose code it is. `None` when
/// the entry would be Ok.
pub(crate) fn planned_code<T: Tree>(
    tree: &mut T,
    file: &str,
    is_pointer: bool,
) -> Result<Option<Planned>, GitError> {
    let (directory, name) = file.rsplit_once('/').unwrap_or(("", file));
    let Descent { at, fault, rest } = descend(tree, directory)?;
    // A directory without a logical path has its own code, so it is the
    // fault; the walk gives no code beneath one that has neither.
    let mut parent = match (fault, at.logical) {
        (None, Some(logical)) => logical,
        (fault, _) => return Ok(fault.map(Planned::Directory)),
    };
    let mut path = at.path;
    // Only the first segment lands in a directory that exists; each later
    // one is alone in a directory the write creates.
    let mut existing = Some(at.directory);
    let last = rest.len();
    for (depth, segment) in rest.into_iter().chain([name]).enumerate() {
        let ours = RawEntry {
            name: segment.as_bytes().to_vec(),
            kind: if depth == last {
                Kind::File(ObjectId::Sha1([0; 20]))
            } else {
                Kind::Directory(tree.root())
            },
            stat: None,
            size: None,
        };
        let directory = existing.take();
        let mut entries = match &directory {
            Some(directory) => tree.alike(directory, &ours.name, address::segment(&ours))?,
            None => Vec::new(),
        };
        entries.retain(|entry| entry.name != ours.name);
        entries.push(ours);
        let mut pointers = Pointers::default();
        let addressed = address::directory(&parent, &entries, |record| match &directory {
            Some(directory) if record.name != segment.as_bytes() => {
                pointers.shadows(record, || tree.read(directory, record))
            }
            _ => Ok(is_pointer),
        });
        // Ours is the last entry addressed. Like a directory, an entry
        // without a logical path has its own code.
        match addressed.into_iter().last() {
            Some((Some(logical), None)) => parent = logical,
            ours => {
                return Ok(ours.and_then(|(_, own)| own).map(|code| {
                    if depth == last {
                        Planned::File(code)
                    } else {
                        Planned::Directory(Beneath::new(code, &path, &entries, segment, true))
                    }
                }));
            }
        }
        path = join(&path, segment);
    }
    Ok(None)
}

/// `segment` inside the `/`-separated `directory`, where empty is the root.
fn join(directory: &str, segment: &str) -> String {
    if directory.is_empty() {
        segment.to_owned()
    } else {
        format!("{directory}/{segment}")
    }
}

fn names_in<T: Tree>(tree: &mut T, path: &str) -> Result<Vec<Name>, GitError> {
    let Descent { at, rest, .. } = descend(tree, path)?;
    let (
        Pending {
            directory,
            logical: Some(parent),
            ..
        },
        true,
    ) = (at, rest.is_empty())
    else {
        return Ok(Vec::new());
    };
    let listed = tree.list(&directory)?;
    let addressed = tree.address(&directory, &parent, &listed);
    Ok(listed
        .iter()
        .zip(addressed)
        .filter_map(|(entry, (logical, _))| {
            Some(Name {
                segment: logical?.segments().last()?.to_owned(),
                directory: matches!(entry.kind, Kind::Directory(_)),
            })
        })
        .collect())
}

pub(crate) fn walk_from<T: Tree>(tree: &mut T, path: &str) -> Result<Vec<Entry>, GitError> {
    let Descent {
        at: start, rest, ..
    } = descend(tree, path)?;
    if !rest.is_empty() {
        return Ok(Vec::new());
    }
    let start_path = start.path.clone();
    let mut entries = Vec::new();
    let mut unhashed = Vec::new();
    let start = (start.directory.clone(), start);
    tree.visit_all(start, &mut |tree, (directory, pending), listed| {
        let listed = match listed {
            Ok(listed) => listed,
            Err(error) if pending.path == start_path => return Err(error),
            Err(error) => {
                let code = pending.inherited.unwrap_or(error.code());
                entries.push(Entry {
                    path: pending.path,
                    logical: pending.logical,
                    blob: None,
                    status: Status::Degraded(code),
                    stat: None,
                    size: None,
                });
                return Ok(Vec::new());
            }
        };
        let addressed = match &pending.logical {
            Some(parent) => tree.address(&directory, parent, &listed),
            None => vec![(None, None); listed.len()],
        };
        let mut children = Vec::new();
        for (entry, (logical, own)) in listed.iter().zip(addressed) {
            let name = String::from_utf8_lossy(&entry.name);
            let path = join(&pending.path, &name);
            let code = pending.inherited.or(own);
            let blob = match &entry.kind {
                Kind::Directory(child) => {
                    let child = child.clone();
                    children.push((
                        child.clone(),
                        Pending {
                            directory: child,
                            path,
                            logical,
                            inherited: code,
                        },
                    ));
                    continue;
                }
                Kind::File(id) | Kind::Symlink(id) => Some(*id),
                Kind::Submodule | Kind::Unreadable => None,
                Kind::Unhashed(seen) => {
                    unhashed.push(Unhashed {
                        at: entries.len(),
                        directory: directory.clone(),
                        name: entry.name.clone(),
                        seen: *seen,
                    });
                    None
                }
            };
            entries.push(Entry {
                path,
                logical,
                blob,
                status: code.map_or(Status::Ok, Status::Degraded),
                stat: entry.stat,
                size: blob.and(entry.size),
            });
        }
        Ok(children)
    })?;
    for (at, kind, stat) in tree.settle_all(unhashed) {
        // `at` is where the walk pushed the entry it listed `Unhashed`.
        let entry = &mut entries[at];
        entry.blob = match kind {
            Kind::File(id) | Kind::Symlink(id) => Some(id),
            Kind::Directory(_) | Kind::Submodule | Kind::Unreadable | Kind::Unhashed(_) => None,
        };
        entry.stat = stat;
        entry.size = entry.blob.and(stat.map(|stat| stat.size));
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

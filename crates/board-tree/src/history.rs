//! A commit named by id, and the commits that changed a path
//! (docs/architecture/records/history.md).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use board_model::ObjectId;
use gix::object::tree::EntryKind;

use crate::git::{GitError, Repository, Sizes, model_id};
use crate::symlink::{self, Chains};

/// The most commits one walk reads, which bounds a walk for a path last
/// changed far back.
pub const MAX_WALKED: usize = 100_000;

/// The most bytes of a commit's subject a change keeps.
const MAX_SUBJECT_BYTES: usize = 256;

/// How far a walk goes: until it holds `limit` changes, when given, and
/// for at most `bound` commits.
#[derive(Clone, Copy, Debug)]
pub struct Walk {
    pub limit: Option<usize>,
    pub bound: usize,
}

/// The commits that changed a path, newest first, and how the walk ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub changes: Vec<Change>,
    /// How many commits the walk read.
    pub walked: usize,
    pub end: End,
}

/// Why a walk stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// It read every commit it could reach, back to each root.
    Roots,
    /// It held its limit of changes with commits still to read.
    Limit,
    /// Older commits are out of its reach: it read its bound of commits, or
    /// met a shallow commit, whose parents the repository lacks.
    Reach,
}

impl History {
    /// Whether the walk stopped before the end of the history, so older
    /// changes may exist.
    pub fn truncated(&self) -> bool {
        self.end != End::Roots
    }

    /// Whether a larger limit could reach every older commit.
    pub fn complete(&self) -> bool {
        self.end != End::Reach
    }
}

/// One commit that changed a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub commit: ObjectId,
    /// The committer time, in seconds since the Unix epoch.
    pub time: i64,
    /// The first line of the message, at most 256 bytes, with bytes that
    /// are not UTF-8 replaced.
    pub subject: String,
    /// The file at the path in that commit, or `None` when the commit
    /// removed it or the path is a directory there.
    pub blob: Option<ObjectId>,
}

/// What the walk does with one commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub(crate) listed: bool,
    pub(crate) follow: Follow,
}

/// Which parents of a commit the walk goes on to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// The parent at this index, the first whose entry equals the commit's.
    One(usize),
    All,
}

/// git's default rule for a path (`git log -- <path>`): a commit whose
/// entry at the path equals a parent's is not listed, and the walk follows
/// the first such parent only. Any other commit is listed, and every parent
/// is walked; a commit with no parent is listed when it has the entry.
pub(crate) fn decide<E: PartialEq>(entry: Option<&E>, parents: &[Option<E>]) -> Decision {
    match parents.iter().position(|parent| parent.as_ref() == entry) {
        Some(first) => Decision {
            listed: false,
            follow: Follow::One(first),
        },
        None => Decision {
            listed: !parents.is_empty() || entry.is_some(),
            follow: Follow::All,
        },
    }
}

/// A tree entry as the walk compares it: its kind and the object it names.
type Tracked = Option<(EntryKind, gix::ObjectId)>;

impl Repository {
    /// `id`, when it names a commit this repository holds. An id of another
    /// object, or one the repository does not hold, or of a length its hash
    /// does not use, is [`GitError::NoCommit`].
    pub fn commit(&self, id: ObjectId) -> Result<ObjectId, GitError> {
        let absent = || GitError::NoCommit {
            repository: self.path().to_path_buf(),
            id,
        };
        let gix_id = self.gix_id(id).map_err(|_| absent())?;
        if gix_id.kind() != self.git().object_hash() {
            return Err(absent());
        }
        match self.git().try_find_header(gix_id) {
            Ok(Some(header)) if header.kind() == gix::object::Kind::Commit => Ok(id),
            Ok(_) => Err(absent()),
            Err(error) => Err(self.missing(id, error)),
        }
    }

    /// The commits that changed what the physical `path` names, newest
    /// first, from `start`. `""` is the root. The walk takes the commit with
    /// the newest committer time from a queue that starts with `start`,
    /// with ties taken by the smaller id, and queues the parents that
    /// git's rule for a path follows. A shallow commit is walked as a root.
    /// A symlink at `path` that leads to a file in `start` stands for that
    /// file, as a read through it does, so the walk follows the file's path.
    pub fn history(&self, start: ObjectId, path: &str, walk: Walk) -> Result<History, GitError> {
        let shallow: HashSet<gix::ObjectId> = (self.git().shallow_commits())
            .map_err(|error| self.missing(start, error))?
            .map(|commits| commits.iter().copied().collect())
            .unwrap_or_default();
        let followed = self.followed(start, path)?;
        let mut walker = Walker {
            repository: self,
            path: &followed,
            entries: HashMap::new(),
        };
        let start = self.gix_id(start)?;
        let mut queue = BinaryHeap::from([(walker.time(start)?, Reverse(start))]);
        let mut queued = HashSet::from([start]);
        let (mut changes, mut read, mut end) = (Vec::new(), 0, End::Roots);
        let mut beyond = false;
        while let Some((time, Reverse(id))) = queue.pop() {
            if read == walk.bound {
                end = End::Reach;
                break;
            }
            read += 1;
            let commit = walker.find(id)?;
            let parents: Vec<gix::ObjectId> = if shallow.contains(&id) {
                beyond = true;
                Vec::new()
            } else {
                commit.parent_ids().map(gix::Id::detach).collect()
            };
            let entry = walker.entry(id)?;
            let before = (parents.iter())
                .map(|parent| walker.entry(*parent))
                .collect::<Result<Vec<_>, _>>()?;
            let Decision { listed, follow } = decide(entry.as_ref(), &before);
            if listed {
                changes.push(Change {
                    commit: model_id(&id),
                    time,
                    subject: subject(commit.message_raw_sloppy()),
                    blob: entry
                        .and_then(|(kind, id)| (kind != EntryKind::Tree).then(|| model_id(&id))),
                });
            }
            let followed = match follow {
                Follow::One(at) => &parents[at..=at],
                Follow::All => &parents[..],
            };
            for parent in followed {
                if queued.insert(*parent) {
                    queue.push((walker.time(*parent)?, Reverse(*parent)));
                }
            }
            if walk.limit.is_some_and(|limit| changes.len() >= limit) {
                if !queue.is_empty() {
                    end = End::Limit;
                }
                break;
            }
        }
        if beyond {
            end = End::Reach;
        }
        Ok(History {
            changes,
            walked: read,
            end,
        })
    }
}

impl Repository {
    /// The physical path of the file a symlink at `path` leads to in
    /// commit `start`, or `path` itself when no symlink there leads to a
    /// file.
    fn followed(&self, start: ObjectId, path: &str) -> Result<String, GitError> {
        let mut tree = self.commit_tree(start)?.sizing(Sizes::Symlinks);
        let mut chains = Chains::default();
        let link = chains.find(&mut tree, path.as_bytes())?;
        Ok(match chains.follow(&mut tree, link)? {
            Some((symlink::End::File { at, .. }, _)) => (chains.names(at).iter())
                .map(|name| String::from_utf8_lossy(name))
                .collect::<Vec<_>>()
                .join("/"),
            _ => path.to_owned(),
        })
    }
}

/// What one walk reads, with each commit's entry at the path read once.
struct Walker<'r> {
    repository: &'r Repository,
    path: &'r str,
    entries: HashMap<gix::ObjectId, Tracked>,
}

impl<'r> Walker<'r> {
    fn find(&self, id: gix::ObjectId) -> Result<gix::Commit<'r>, GitError> {
        let repository = self.repository;
        (repository.git().find_commit(id)).map_err(|error| repository.missing(model_id(&id), error))
    }

    /// The committer time of commit `id`.
    fn time(&self, id: gix::ObjectId) -> Result<i64, GitError> {
        let time = self.find(id)?.time();
        time.map(|time| time.seconds)
            .map_err(|error| self.repository.missing(model_id(&id), error))
    }

    /// The entry at the path in commit `id`'s tree.
    fn entry(&mut self, id: gix::ObjectId) -> Result<Tracked, GitError> {
        if let Some(entry) = self.entries.get(&id) {
            return Ok(*entry);
        }
        let repository = self.repository;
        let missing = |error: Box<dyn std::error::Error + Send + Sync>| {
            repository.missing(model_id(&id), error)
        };
        let tree = self
            .find(id)?
            .tree()
            .map_err(|error| missing(error.into()))?;
        let entry = if self.path.is_empty() {
            Some((EntryKind::Tree, tree.id))
        } else {
            (tree.lookup_entry(self.path.split('/')))
                .map_err(|error| missing(error.into()))?
                .map(|entry| (entry.mode().kind(), entry.object_id()))
        };
        self.entries.insert(id, entry);
        Ok(entry)
    }
}

/// The first line of `message`, cut at [`MAX_SUBJECT_BYTES`] on a
/// character boundary.
fn subject(message: &gix::bstr::BStr) -> String {
    let line = message
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let mut subject = String::from_utf8_lossy(line).into_owned();
    if subject.len() > MAX_SUBJECT_BYTES {
        let mut end = MAX_SUBJECT_BYTES;
        while !subject.is_char_boundary(end) {
            end -= 1;
        }
        subject.truncate(end);
    }
    subject
}

//! The one module that writes the working tree and the git directory
//! (docs/architecture.md, Writes): where a put lands, the lock, and the
//! write that a kill cannot leave half done.
#![expect(
    clippy::disallowed_methods,
    reason = "board-tree's write module owns every write to the working tree and the git directory"
)]

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{Code, Location, LogicalPath, MAX_RECORD_BYTES, Object, ObjectId, Source};

use crate::address;
use crate::git::{GitError, Repository, model_id};
use crate::resolve::ResolveError;
use crate::tree::{Kind, RawEntry, Tree};

/// How long a put waits for another put's lock.
pub const LOCK_WAIT: Duration = Duration::from_secs(5);
/// How often a waiting put tries the lock again.
const LOCK_POLL: Duration = Duration::from_millis(20);

/// Where a put writes: the record file a logical path names, existing or
/// not, in the working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The file's path in the working tree, `/`-separated.
    pub path: String,
    /// The Location resolution reached, with every pointer followed; for a
    /// new file, its parent directory's.
    pub location: Location,
}

/// The lock a put holds from comparing its `--expect` to renaming its file.
/// Dropping it removes the lock file.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        // A lock that cannot be removed is reported by the next put.
        let _ = fs::remove_file(&self.path);
    }
}

impl Repository {
    /// The record file `path` names for writing. Pointers are followed,
    /// including a link at the last segment; a path that reaches a mount is
    /// refused. A path that names nothing yet names a new file beneath the
    /// deepest directory that exists. `is_pointer` says whether the record
    /// to be written is a pointer, which would shadow a directory beside it;
    /// a write that would leave its own path degraded is refused with that
    /// path's code.
    pub fn target(&self, path: &LogicalPath, is_pointer: bool) -> Result<Target, WriteError> {
        let refused = |code| WriteError::Refused {
            path: path.clone(),
            code,
        };
        let (location, physical) = match self.resolve(&Source::WorkingTree, path) {
            Ok(location) => {
                let physical = record_file(&location).ok_or(refused(Code::RecordNotARecord))?;
                (location, physical)
            }
            Err(ResolveError::NotFound { .. }) => self.new_file(path)?,
            Err(error) => return Err(refused(error.code())),
        };
        if location.board.source != Source::WorkingTree {
            return Err(refused(Code::PutThroughMount));
        }
        if let Some(code) = self.degraded(&physical, is_pointer)? {
            return Err(refused(code));
        }
        Ok(Target {
            path: physical,
            location,
        })
    }

    /// The new file `path` names: beneath its deepest ancestor that
    /// resolves, with the segments past it as directories to create.
    fn new_file(&self, path: &LogicalPath) -> Result<(Location, String), WriteError> {
        let segments: Vec<&str> = path.segments().collect();
        for depth in (0..segments.len()).rev() {
            let ancestor: LogicalPath = format!("/{}", segments[..depth].join("/"))
                .parse()
                .map_err(|_| WriteError::Refused {
                    path: path.clone(),
                    code: Code::PathMalformed,
                })?;
            match self.resolve(&Source::WorkingTree, &ancestor) {
                Ok(location) => {
                    let directory = match location.object {
                        Object::Directory => location.path.clone(),
                        // A record gains children in a directory beside it.
                        Object::Blob(_) => match location.path.strip_suffix(".json") {
                            Some(stem) => stem.to_owned(),
                            None => {
                                return Err(WriteError::Refused {
                                    path: path.clone(),
                                    code: Code::RecordNotARecord,
                                });
                            }
                        },
                    };
                    let rest = &segments[depth..];
                    let mut physical: Vec<String> = Vec::new();
                    if !directory.is_empty() {
                        physical.push(directory);
                    }
                    physical.extend(rest.iter().map(|segment| (*segment).to_owned()));
                    let file = format!("{}.json", physical.join("/"));
                    return Ok((location, file));
                }
                Err(ResolveError::NotFound { .. }) => {}
                Err(error) => {
                    return Err(WriteError::Refused {
                        path: path.clone(),
                        code: error.code(),
                    });
                }
            }
        }
        Err(WriteError::Refused {
            path: path.clone(),
            code: Code::PathNotFound,
        })
    }

    /// The code the entry for `file` would have in the deepest directory of
    /// its path that exists, once written: the new file itself, or the first
    /// directory to be created on the way to it.
    fn degraded(&self, file: &str, is_pointer: bool) -> Result<Option<Code>, WriteError> {
        let mut known = |_: &str, _: &board_model::Stat| None;
        let mut tree = self.working_tree(&mut known)?;
        let mut directory = PathBuf::new();
        let mut parts = file.split('/').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let listed = tree.list(&directory)?;
            let exists = listed.iter().any(|entry| entry.name == part.as_bytes());
            if exists && !last {
                directory.push(part);
                continue;
            }
            let mut entries: Vec<RawEntry<PathBuf>> = listed
                .iter()
                .filter(|entry| entry.name != part.as_bytes())
                .cloned()
                .collect();
            entries.push(RawEntry {
                name: part.as_bytes().to_vec(),
                kind: if last {
                    Kind::File(ObjectId::Sha1([0; 20]))
                } else {
                    Kind::Directory(directory.join(part))
                },
                stat: None,
                size: None,
            });
            let ours = entries.len() - 1;
            let addressed = address::directory(&LogicalPath::root(), &entries, |record| {
                if record.name == part.as_bytes() {
                    Some(is_pointer)
                } else {
                    let read = tree.read(&directory, record).ok()?;
                    Some(
                        read.is_some_and(|bytes| !matches!(board_model::pointer(&bytes), Ok(None))),
                    )
                }
            });
            return Ok(addressed.get(ours).and_then(|(_, code)| *code));
        }
        Ok(None)
    }

    /// The bytes of the working-tree file at the `/`-separated `path`, at
    /// most one byte past [`MAX_RECORD_BYTES`] so a parse sees it is too
    /// large, or `None` when there is none.
    pub fn read_working_file(&self, path: &str) -> Result<Option<Vec<u8>>, WriteError> {
        let absolute = self.path().join(path);
        let read = fs::File::open(&absolute).and_then(|file| {
            let mut bytes = Vec::new();
            file.take(MAX_RECORD_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        match read {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(WriteError::Io {
                path: absolute,
                source,
            }),
        }
    }

    /// The git directory of this working tree, where its locks live.
    pub fn git_dir(&self) -> &Path {
        self.git().git_dir()
    }

    /// Takes `board.lock` in the git directory, created exclusively as git
    /// creates its lock files, trying again until `wait` has passed.
    pub fn lock(&self, wait: Duration) -> Result<Lock, WriteError> {
        let path = self.git_dir().join("board.lock");
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Lock { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() >= wait {
                        return Err(WriteError::Locked { lock: path });
                    }
                    std::thread::sleep(LOCK_POLL);
                }
                Err(source) => return Err(WriteError::Io { path, source }),
            }
        }
    }

    /// The blob of the working-tree file at the `/`-separated `path`, or
    /// `None` when there is none. Read in full: a put compares it.
    pub fn current_blob(&self, path: &str) -> Result<Option<ObjectId>, WriteError> {
        let absolute = self.path().join(path);
        match fs::read(&absolute) {
            Ok(bytes) => Ok(Some(self.blob_id(&bytes))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(WriteError::Io {
                path: absolute,
                source,
            }),
        }
    }

    /// The id git gives a blob holding `bytes`.
    ///
    /// # Panics
    ///
    /// Never: hashing bytes in memory reads no stream.
    pub fn blob_id(&self, bytes: &[u8]) -> ObjectId {
        let id = gix::objs::compute_hash(self.git().object_hash(), gix::objs::Kind::Blob, bytes)
            .expect("hashing bytes in memory reads no stream, so it cannot fail");
        model_id(&id)
    }

    /// Writes `bytes` to the working-tree file at the `/`-separated `path`
    /// so that a kill leaves the old file or the new one: to a temporary
    /// file beside it, fsynced, renamed into place, and the directory
    /// fsynced. Missing directories are created; an existing file keeps its
    /// permissions. The lock must be held.
    pub fn write_file(&self, _lock: &Lock, path: &str, bytes: &[u8]) -> Result<(), WriteError> {
        let target = self.path().join(path);
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| WriteError::Io { path, source }
        };
        let directory = target.parent().unwrap_or(self.path()).to_path_buf();
        fs::create_dir_all(&directory).map_err(io(&directory))?;
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = directory.join(format!(".{name}.board-tmp"));
        let written = write_temporary(&temporary, &target, bytes)
            .and_then(|()| fs::rename(&temporary, &target));
        if let Err(source) = written {
            let _ = fs::remove_file(&temporary);
            return Err(WriteError::Io {
                path: target,
                source,
            });
        }
        fs::File::open(&directory)
            .and_then(|directory| directory.sync_all())
            .map_err(io(&directory))
    }
}

/// Writes and fsyncs the temporary file, with the target's permissions when
/// the target exists.
fn write_temporary(temporary: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(temporary)?;
    file.write_all(bytes)?;
    if let Ok(metadata) = fs::metadata(target) {
        file.set_permissions(metadata.permissions())?;
    }
    file.sync_all()
}

/// The record file a resolved Location writes to: the file itself, or the
/// record beside a directory. `None` when it is a file that is not a record,
/// or the root.
fn record_file(location: &Location) -> Option<String> {
    match location.object {
        Object::Blob(_) => board_model::needs_bytes(&location.path).then(|| location.path.clone()),
        Object::Directory if location.path.is_empty() => None,
        Object::Directory => Some(format!("{}.json", location.path)),
    }
}

/// A write that cannot be made.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{path} cannot be written: {code}")]
    Refused { path: LogicalPath, code: Code },
    #[error("{lock:?} is held; if no put is running, remove it")]
    Locked { lock: PathBuf },
    #[error("{path:?} cannot be written")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Git(#[from] GitError),
}

impl WriteError {
    pub fn code(&self) -> Code {
        match self {
            Self::Refused { code, .. } => *code,
            Self::Locked { .. } => Code::PutLocked,
            Self::Io { .. } => Code::RepoUnreadable,
            Self::Git(error) => error.code(),
        }
    }
}

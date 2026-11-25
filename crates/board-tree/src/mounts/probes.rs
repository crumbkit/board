//! What opening a mounted repository found, kept under a stamp of the files
//! that decide it, so a run opens the repository only when one of them
//! changed (docs/architecture/performance/warm/index.md).

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use board_model::{Code, Digest, LogicalPath, Mount, ObjectId, Source};

use super::keys::{Probe, hashed};
use super::{Cache, Candidate, Edge, MountPoint, Reached};
use crate::git::{Identity, Repository};
use crate::repositories::Repositories;
use crate::tree::Tree;

/// What opening a mounted repository and finding its pinned commit found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probed {
    /// The repository's path, as [`Repository::path`] gives it.
    pub path: PathBuf,
    /// The pinned commit's root tree.
    pub root: ObjectId,
    /// Its common git directory, canonical, when it has one.
    pub common: Option<PathBuf>,
    /// The stored form of its `origin`, when git can read one.
    pub origin: Option<String>,
}

impl Probed {
    fn of(repository: &Repository, root: ObjectId) -> Self {
        let Identity { common, origin } = repository.identity();
        Self {
            path: repository.path().to_path_buf(),
            root,
            common,
            origin,
        }
    }

    fn identity(&self) -> Identity {
        Identity {
            common: self.common.clone(),
            origin: self.origin.clone(),
        }
    }
}

/// The most levels of `include.path` a stamp follows, as git allows.
const MAX_INCLUDES: usize = 10;

/// The stamp fields every mount shares: the stat of each git config file
/// the root repository read that is not its own, and of each file those
/// include under any condition, and the text of each value that came from
/// no file, such as the environment's. `None` when one cannot be read.
pub(super) fn shared(root: &Repository) -> Option<Vec<Vec<u8>>> {
    use gix::config::Source as From;
    let config = root.git().config_snapshot();
    let mut fields = Vec::new();
    let mut files = Vec::new();
    for section in config.plumbing().sections() {
        let meta = section.meta();
        if matches!(meta.source, From::Local | From::Worktree) {
            continue;
        }
        match &meta.path {
            Some(path) if meta.level == 0 => files.push(path.clone()),
            Some(_) => {}
            None => fields.push(section.to_bstring().to_vec()),
        }
    }
    let mut stamp = Stamp(fields);
    let mut seen = HashSet::new();
    for file in files {
        stamp.config(&file, &mut seen, 0)?;
    }
    Some(stamp.0)
}

/// The stamp of the repository at `named` for `commit`, beside `shared`:
/// its path and canonical path, its `.git`, git directory and common
/// directory, its own config files and what they include, its alternates,
/// its pack directory and the commit's loose object. `None` when the path
/// does not resolve or a file cannot be read for a reason other than its
/// absence, so the repository is opened.
fn stamp(named: &Path, commit: ObjectId, shared: &[Vec<u8>]) -> Option<Digest> {
    let canonical = named.canonicalize().ok()?;
    let mut stamp = Stamp(vec![
        b"probe".to_vec(),
        named.as_os_str().as_encoded_bytes().to_vec(),
        commit.to_string().into_bytes(),
        canonical.as_os_str().as_encoded_bytes().to_vec(),
    ]);
    stamp.0.extend_from_slice(shared);
    let (git_dir, common) = stamp.located(&canonical)?;
    let mut seen = HashSet::new();
    stamp.config(&common.join("config"), &mut seen, 0)?;
    stamp.config(&git_dir.join("config.worktree"), &mut seen, 0)?;
    let objects = common.join("objects");
    let hex = commit.to_string();
    stamp.file(&objects.join("info/alternates"))?;
    stamp.file(&objects.join("pack"))?;
    stamp.file(&objects.join(&hex[..2]).join(&hex[2..]))?;
    Some(hashed(&stamp.0))
}

/// The stamp of the objects the repository at `named` holds: its pack
/// directory and each directory of loose objects, whose times change when
/// a blob arrives by a fetch or a loose write. `None` when the path does
/// not resolve, a file cannot be read for a reason other than its absence,
/// or the repository borrows objects through alternates, whose stores this
/// does not stamp.
pub(super) fn objects(named: &Path) -> Option<Vec<Vec<u8>>> {
    let canonical = named.canonicalize().ok()?;
    let mut stamp = Stamp(vec![
        b"objects".to_vec(),
        canonical.as_os_str().as_encoded_bytes().to_vec(),
    ]);
    let (_, common) = stamp.located(&canonical)?;
    let objects = common.join("objects");
    match fs::symlink_metadata(objects.join("info/alternates")) {
        Err(error) if absent(&error) => {}
        _ => return None,
    }
    stamp.file(&objects.join("pack"))?;
    for fanout in 0..=u8::MAX {
        stamp.file(&objects.join(format!("{fanout:02x}")))?;
    }
    Some(stamp.0)
}

/// Whether a read failed because the file, or a directory above it, is
/// not there.
fn absent(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// The fields of a stamp, in the order they were read.
struct Stamp(Vec<Vec<u8>>);

impl Stamp {
    /// The git directory and common directory of the repository whose
    /// canonical path is `canonical`, with what was read to find them. A
    /// `.git` file names a linked worktree's git directory; a `.git`
    /// directory is the git directory; with neither, the repository is
    /// bare.
    fn located(&mut self, canonical: &Path) -> Option<(PathBuf, PathBuf)> {
        let dot_git = canonical.join(".git");
        let git_dir = match fs::read_to_string(&dot_git) {
            Ok(text) => {
                self.file(&dot_git)?;
                canonical.join(text.strip_prefix("gitdir:")?.trim())
            }
            Err(error) if error.kind() == ErrorKind::IsADirectory => dot_git,
            Err(error) if absent(&error) => canonical.to_path_buf(),
            Err(_) => return None,
        };
        let git_dir = git_dir.canonicalize().ok()?;
        self.directory(&git_dir)?;
        let commondir = git_dir.join("commondir");
        self.file(&commondir)?;
        let common = match fs::read_to_string(&commondir) {
            Ok(text) => git_dir.join(text.trim()).canonicalize().ok()?,
            Err(error) if absent(&error) => git_dir.clone(),
            Err(_) => return None,
        };
        self.directory(&common)?;
        Some((git_dir, common))
    }

    /// The path and stat of the file at `path`, followed through symlinks:
    /// its size, times, inode, device and mode, or that it is absent.
    fn file(&mut self, path: &Path) -> Option<()> {
        self.0.push(path.as_os_str().as_encoded_bytes().to_vec());
        match fs::metadata(path) {
            Ok(meta) => {
                let numbers = [
                    meta.size(),
                    meta.mtime().cast_unsigned(),
                    meta.mtime_nsec().cast_unsigned(),
                    meta.ctime().cast_unsigned(),
                    meta.ctime_nsec().cast_unsigned(),
                    meta.ino(),
                    meta.dev(),
                    u64::from(meta.mode()),
                ];
                self.0
                    .push(numbers.iter().flat_map(|n| n.to_le_bytes()).collect());
            }
            Err(error) if absent(&error) => self.0.push(b"absent".to_vec()),
            Err(_) => return None,
        }
        Some(())
    }

    /// The path, inode and device of the directory at `path`, which say
    /// which directory it is. Its times change with every commit, so they
    /// are left out.
    fn directory(&mut self, path: &Path) -> Option<()> {
        let meta = fs::metadata(path).ok()?;
        self.0.push(path.as_os_str().as_encoded_bytes().to_vec());
        let numbers = [meta.ino(), meta.dev()];
        self.0
            .push(numbers.iter().flat_map(|n| n.to_le_bytes()).collect());
        Some(())
    }

    /// The config file at `path`, and each file its `include` and
    /// `includeIf` sections name, under any condition.
    fn config(&mut self, path: &Path, seen: &mut HashSet<PathBuf>, depth: usize) -> Option<()> {
        if depth > MAX_INCLUDES || !seen.insert(path.to_path_buf()) {
            return Some(());
        }
        self.file(path)?;
        let text = match fs::read(path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if absent(&error) => return Some(()),
            Err(_) => return None,
        };
        let directory = path.parent().unwrap_or(Path::new("/"));
        for include in includes(&text) {
            let named = match include.strip_prefix("~/") {
                Some(rest) => home(path)?.join(rest),
                None => directory.join(include),
            };
            self.config(&named, seen, depth + 1)?;
        }
        Some(())
    }
}

/// The home directory a config file at `path` names with `~/`: the one
/// gix read it from when it is the user's own, else none is known.
fn home(path: &Path) -> Option<PathBuf> {
    gix::path::env::home_dir().filter(|home| path.starts_with(home))
}

/// The `path` values of every `include` and `includeIf` section of the
/// config `text`.
fn includes(text: &str) -> Vec<String> {
    let mut within = false;
    let mut found = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            let name = header.split([']', ' ', '"', '.', '\t']).next();
            let name = name.unwrap_or_default().to_ascii_lowercase();
            within = name == "include" || name == "includeif";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| within) else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("path") {
            found.push(value.trim().trim_matches('"').to_owned());
        }
    }
    found
}

/// The repository `mount`, a pointer in the repository at `from`, names,
/// opened, with its pinned commit's root tree, or why it cannot be read.
fn opened(repositories: &Repositories, from: &Path, mount: &Mount) -> Result<Probed, Code> {
    let mounted = repositories
        .mounted(from, mount)
        .ok_or(Code::MountRepoMissing)?;
    let tree = mounted.commit_tree(mount.commit);
    let root = tree.map_err(|_| Code::MountCommitMissing)?.root();
    Ok(Probed::of(&mounted, root))
}

/// The cache a walk through mounts reads probes from, with the stamp
/// fields every mount shares, or `None` when probes are not kept.
pub(super) type Probing<'c, C> = (&'c mut C, Option<&'c [Vec<u8>]>);

impl<F> Reached<F> {
    /// Records the mounts `found` in `source` of the repository at `from`,
    /// whose route is `route` and whose `chain` ends with its own board, and
    /// adds each one that can be read, whose route is a logical path and
    /// whose repository is on no board of `chain`, as a candidate. A mount
    /// whose stamp the cache holds a probe under is not opened; one that is
    /// opened and found keeps its probe in the cache.
    pub(super) fn add<C: Cache>(
        &mut self,
        (repositories, (cache, shared)): (&Repositories, Probing<'_, C>),
        from: &Path,
        (source, route, chain): (&Source, &LogicalPath, &[(Identity, LogicalPath)]),
        found: Vec<(LogicalPath, String, Mount)>,
        candidates: &mut Vec<Candidate>,
    ) -> Result<(), C::Error> {
        let found: Vec<_> = (found.into_iter())
            .map(|(at, path, mount)| (at, path.clone(), repositories.read_as(source, &path, mount)))
            .collect();
        let stamped: Vec<(Option<Digest>, Option<Probed>)> = (found.iter())
            .map(|(at, _, mount)| {
                let named = (route.beneath(at).ok()).and_then(|_| repositories.named(from, mount));
                let stamp = (named.zip(shared))
                    .and_then(|(named, shared)| stamp(&named, mount.commit, shared));
                let kept = stamp.as_ref().and_then(|stamp| cache.probe(stamp));
                (stamp, kept)
            })
            .collect();
        let wanted: Vec<_> = (found.iter().zip(&stamped))
            .filter(|((at, _, _), (_, kept))| kept.is_none() && route.beneath(at).is_ok())
            .filter_map(|((_, _, mount), _)| Some((repositories.named(from, mount)?, mount.commit)))
            .collect();
        repositories.open_all(&wanted);
        for ((at, path, mount), (stamp, kept)) in found.into_iter().zip(stamped) {
            let probed = match (route.beneath(&at), kept) {
                (Err(_), _) => Err(Code::PathMalformed),
                (Ok(route), Some(kept)) => Ok((route, kept)),
                (Ok(route), None) => {
                    let probed = opened(repositories, from, &mount);
                    if let (Ok(probed), Some(stamp)) = (&probed, &stamp) {
                        cache.put_probe(stamp, probed)?;
                    }
                    probed.map(|probed| (route, probed))
                }
            };
            let identity = (probed.as_ref().ok()).map(|(_, probed)| probed.identity());
            let repeats = identity.as_ref().and_then(|identity| {
                let above = chain.iter().find(|(above, _)| above.same(identity));
                above.map(|(_, repeats)| repeats.clone())
            });
            let probed = match repeats {
                Some(repeats) => {
                    self.cycles.insert((source.clone(), path.clone()), repeats);
                    Err(Code::MountCycle)
                }
                None => probed,
            };
            self.probes.push(Probe {
                from: source.clone(),
                path: path.clone(),
                repo: mount.repo.clone(),
                commit: mount.commit,
                landed: (probed.as_ref())
                    .map(|(_, probed)| probed.path.clone())
                    .map_err(|code| *code),
            });
            self.points.push(MountPoint {
                source: source.clone(),
                path,
                target: probed.as_ref().map(|_| mount.commit).map_err(|code| *code),
            });
            let (Ok((route, probed)), Some(identity)) = (probed, identity) else {
                continue;
            };
            let mut below = chain.to_vec();
            below.push((identity, route.clone()));
            candidates.push(Candidate {
                route,
                commit: mount.commit,
                root: probed.root,
                repository: probed.path,
                chain: below,
            });
            self.edges.push(Edge {
                from: source.clone(),
                at,
                to: mount.commit,
            });
        }
        Ok(())
    }
}

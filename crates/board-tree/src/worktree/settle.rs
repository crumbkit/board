//! Hashing the files a walk listed without hashing, on several threads.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use board_model::{MAX_RECORD_BYTES, ObjectId, Role, Stat};
use gix::bstr::ByteSlice;

use crate::git::{Contents, model_id};
use crate::tree::{Kind, Settled, Unhashed};
use crate::worktree::{Kept, WorkingTree, read_exactly, stat};
use crate::write::blob_id;

/// One file to hash: which entry it is, its path relative to the root, its
/// `/`-separated path, and the metadata it was seen with.
struct Job {
    at: usize,
    relative: PathBuf,
    path: String,
    seen: Stat,
}

/// What hashing a file needs, which every thread can share: the root, the
/// object hash and the callback, and nothing of the repository's own.
#[derive(Clone, Copy)]
pub(super) struct Hasher<'h> {
    root: &'h Path,
    kind: gix::hash::Kind,
    kept: Option<Kept<'h>>,
}

impl WorkingTree<'_, '_> {
    pub(super) fn hasher(&self) -> Hasher<'_> {
        Hasher {
            root: self.repository.path(),
            kind: self.repository.git().object_hash(),
            kept: self.kept,
        }
    }

    /// Settles each file in `unhashed`. One already settled is answered on
    /// this thread; the rest are opened and hashed on the repository's
    /// threads, no more than there are files, each file on one of them. The
    /// answer is the same however many threads hash.
    pub(super) fn settle_each(
        &mut self,
        unhashed: Vec<Unhashed<PathBuf>>,
    ) -> Vec<Settled<PathBuf>> {
        let mut settled = Vec::with_capacity(unhashed.len());
        let mut jobs = Vec::new();
        for Unhashed {
            at,
            directory,
            name,
            seen,
        } in unhashed
        {
            let relative = directory.join(gix::path::from_bstr(name.as_bstr()));
            if let Some((kind, stat)) = self.settled.get(&relative) {
                settled.push((at, kind.clone(), *stat));
                continue;
            }
            let path = gix::path::into_bstr(&relative).to_str_lossy().into_owned();
            jobs.push(Job {
                at,
                relative,
                path,
                seen,
            });
        }
        self.repository.opened_many(jobs.len());
        let threads = self.repository.threads().min(jobs.len());
        settled.extend(self.hasher().all(&jobs, threads));
        settled
    }
}

impl Hasher<'_> {
    /// Hashes `jobs` on `threads` threads, each taking the next job as it
    /// finishes one; no thread is started for no jobs.
    ///
    /// # Panics
    ///
    /// When a thread panics, with its payload, as if the panic were here.
    fn all(self, jobs: &[Job], threads: usize) -> Vec<Settled<PathBuf>> {
        let next = AtomicUsize::new(0);
        let work = || {
            let mut mine = Vec::new();
            while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                let (kind, stat) = self.hash(&job.relative, &job.path, job.seen);
                mine.push((job.at, kind, stat));
            }
            mine
        };
        thread::scope(|scope| {
            let workers: Vec<_> = (0..threads).map(|_| scope.spawn(work)).collect();
            workers
                .into_iter()
                .flat_map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
                })
                .collect()
        })
    }

    /// Opens and hashes the file at `relative`, whose `/`-separated path is
    /// `path`, that was `seen` with its metadata.
    pub(super) fn hash(
        self,
        relative: &Path,
        path: &str,
        seen: Stat,
    ) -> (Kind<PathBuf>, Option<Stat>) {
        // The file hashed is the one opened, so its own metadata is kept. A
        // file it cannot open keeps the stat it was seen with, so a walk that
        // again cannot open it records no change.
        let absolute = self.root.join(relative);
        let Ok((stat, file)) =
            fs::File::open(&absolute).and_then(|file| Ok((stat(&file.metadata()?), file)))
        else {
            return (Kind::Unreadable, Some(seen));
        };
        // Only a walk that keeps bytes reads a file in full.
        let blob = match self.kept {
            Some(kept) if Role::of(path).is_some() && stat.size <= MAX_RECORD_BYTES as u64 => {
                hash_kept(self.kind, kept, path, file, stat.size)
            }
            _ => hash_stream(self.kind, &mut BufReader::new(file), stat.size),
        };
        (blob.map_or(Kind::Unreadable, Kind::File), Some(stat))
    }
}

/// The id of the `length` bytes of `file`, read in full and handed to `kept`,
/// so the blob is the blob of the bytes kept; `None` if the file cannot be
/// read or does not hold `length` bytes.
fn hash_kept(
    kind: gix::hash::Kind,
    kept: Kept<'_>,
    path: &str,
    file: fs::File,
    length: u64,
) -> Option<ObjectId> {
    let bytes = read_exactly(file, length)?;
    let blob = blob_id(kind, &bytes);
    kept(path, Contents { blob, bytes });
    Some(blob)
}

/// The id git gives a blob of `length` bytes read from `stream`, in constant
/// memory; `None` if the stream fails, or yields fewer or more bytes than
/// `length` because the file changed while it was read.
pub(super) fn hash_stream(
    kind: gix::hash::Kind,
    stream: &mut dyn Read,
    length: u64,
) -> Option<ObjectId> {
    let id = gix::objs::compute_stream_hash(
        kind,
        gix::objs::Kind::Blob,
        stream,
        length,
        &mut gix::features::progress::Discard,
        &AtomicBool::new(false),
    )
    .ok()?;
    let mut more = [0; 1];
    (stream.read(&mut more).ok()? == 0).then(|| model_id(&id))
}

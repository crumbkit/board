//! Hashing the files a walk listed without hashing, on several threads.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;

use board_model::{MAX_RECORD_BYTES, ObjectId, Role, Stat};
use gix::bstr::ByteSlice;

use crate::git::{Contents, model_id};
use crate::tree::{Kind, Settled, Unhashed};
use crate::worktree::{KEPT_AHEAD, Prepare, WorkingTree, read_exactly, stat};
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
pub(super) struct Hasher<'h, T> {
    root: &'h Path,
    kind: gix::hash::Kind,
    prepare: Option<Prepare<'h, T>>,
}

impl<T> WorkingTree<'_, '_, T> {
    pub(super) fn hasher(&self) -> Hasher<'_, T> {
        Hasher {
            root: self.repository.path(),
            kind: self.repository.git().object_hash(),
            prepare: self.kept.as_ref().map(|kept| kept.prepare),
        }
    }
}

impl<T: Send> WorkingTree<'_, '_, T> {
    /// Settles each file in `unhashed`, as [`Tree::settle`] would, so that
    /// each keeps one blob in this snapshot. One already settled is answered
    /// on this thread; the rest are opened and hashed on the repository's
    /// threads, no more than there are files, each file on one of them. The
    /// answer is the same however many threads hash.
    ///
    /// [`Tree::settle`]: crate::tree::Tree::settle
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
        let hasher = Hasher {
            root: self.repository.path(),
            kind: self.repository.git().object_hash(),
            prepare: self.kept.as_ref().map(|kept| kept.prepare),
        };
        let answers = match &mut self.kept {
            None => hasher.all(&jobs, threads, &mut |_| true),
            // A walk that `keep` ended is discarded, so it hashes no more.
            Some(kept) if kept.stopped => Vec::new(),
            Some(kept) => hasher.all(&jobs, threads, &mut |prepared| {
                kept.stopped = !(kept.keep)(prepared);
                !kept.stopped
            }),
        };
        for (job, kind, stat) in answers {
            let Job { at, relative, .. } = &jobs[job];
            self.settled.insert(relative.clone(), (kind.clone(), stat));
            settled.push((*at, kind, stat));
        }
        settled
    }
}

impl<T: Send> Hasher<'_, T> {
    /// Hashes `jobs` on `threads` threads, each taking the next job as it
    /// finishes one, and hands what they prepare to `keep` on this thread
    /// as they prepare it; no thread is started for no jobs. Each answer
    /// names its job by where it is in `jobs`. When `keep`
    /// answers `false`, each thread stops once it next prepares a file.
    ///
    /// # Panics
    ///
    /// When a thread panics, with its payload, as if the panic were here.
    fn all(
        &self,
        jobs: &[Job],
        threads: usize,
        keep: &mut dyn FnMut(T) -> bool,
    ) -> Vec<Settled<PathBuf>> {
        let next = AtomicUsize::new(0);
        let work = |send: SyncSender<T>| {
            let mut mine = Vec::new();
            loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(at) else { break };
                let (kind, stat, prepared) = self.hash(&job.relative, &job.path, job.seen);
                mine.push((at, kind, stat));
                if let Some(prepared) = prepared
                    && send.send(prepared).is_err()
                {
                    break;
                }
            }
            mine
        };
        let (send, prepared) = mpsc::sync_channel(KEPT_AHEAD);
        thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    let send = send.clone();
                    scope.spawn(|| work(send))
                })
                .collect();
            drop(send);
            // The loop ends once every thread has ended and dropped its
            // sender, or when `keep` ends the walk. Either way, or when
            // `keep` panics, the receiver is dropped here, so a thread
            // waiting to send stops instead of waiting forever.
            for prepared in prepared {
                if !keep(prepared) {
                    break;
                }
            }
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
    /// What `prepare` made of the file, if it was read in full, comes back
    /// with its blob.
    pub(super) fn hash(
        &self,
        relative: &Path,
        path: &str,
        seen: Stat,
    ) -> (Kind<PathBuf>, Option<Stat>, Option<T>) {
        // The file hashed is the one opened, so its own metadata is kept. A
        // file it cannot open keeps the stat it was seen with, so a walk that
        // again cannot open it records no change.
        let absolute = self.root.join(relative);
        let Ok((stat, file)) =
            fs::File::open(&absolute).and_then(|file| Ok((stat(&file.metadata()?), file)))
        else {
            return (Kind::Unreadable, Some(seen), None);
        };
        // Only a walk that keeps bytes reads a file in full.
        let (blob, prepared) = match self.prepare {
            Some(prepare) if Role::of(path).is_some() && stat.size <= MAX_RECORD_BYTES as u64 => {
                hash_kept(self.kind, prepare, path, file, stat.size)
                    .map_or((None, None), |(blob, prepared)| {
                        (Some(blob), Some(prepared))
                    })
            }
            _ => (
                hash_stream(self.kind, &mut BufReader::new(file), stat.size),
                None,
            ),
        };
        (
            blob.map_or(Kind::Unreadable, Kind::File),
            Some(stat),
            prepared,
        )
    }
}

/// The id of the `length` bytes of `file`, read in full, and what `prepare`
/// made of them, so the blob is the blob of the bytes prepared; `None` if
/// the file cannot be read or does not hold `length` bytes.
fn hash_kept<T>(
    kind: gix::hash::Kind,
    prepare: Prepare<'_, T>,
    path: &str,
    file: fs::File,
    length: u64,
) -> Option<(ObjectId, T)> {
    let bytes = read_exactly(file, length)?;
    let blob = blob_id(kind, &bytes);
    Some((blob, prepare(path, Contents { blob, bytes })))
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

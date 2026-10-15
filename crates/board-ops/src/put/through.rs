//! A put through one mount: an ordinary put in the mounted board, then the
//! mount's pin moved to the commit it made (docs/architecture/writes.md).

use std::time::Duration;

use board_model::{BINARY_NAME, Location, LogicalPath, Object, ObjectId, Source, repinned};
use board_tree::{Inside, Lock, PinError, Through, WriteError};

use crate::board::Board;
use crate::put::{Expect, Failure, Prepared, PutError, PutReport, Schemas, Written};

impl Board {
    /// Puts `bytes` at `path`, which leads through the mount at `mount` to
    /// `inside`, holding this board's `lock`, and waiting up to `wait` for
    /// the mounted board's. `expect` is compared with the pinned blob.
    pub(super) fn put_through(
        &self,
        lock: Lock,
        path: &LogicalPath,
        (mount, inside): (&LogicalPath, &Inside),
        expect: Expect,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        if inside.repository == self.repository().path() {
            return Err(refused(path, mount, Through::Own));
        }
        let home = self.repositories().home().map(Into::into);
        let mounted = match Board::open(&inside.repository, home) {
            Ok(mounted) if mounted.repository().path() == inside.repository => mounted,
            _ => return Err(refused(path, mount, Through::NotABoard)),
        };
        // Refuses a `follow` that names nothing, as `mount update` does.
        let pin = self.repositories().pin(mount)?;
        let (prepared, pinned, new) =
            mounted.put_mounted(path, (mount, inside), &pin.follow, expect, bytes, wait)?;
        // Repinned even when the pointer already names `new`, so a pin moved
        // by a put killed before the root committed it is committed now.
        let canonical = repinned(&pin.canonical, new).map_err(|error| PinError::Mount {
            path: mount.clone(),
            code: error.code(),
        })?;
        let message = format!("{BINARY_NAME} put {path}");
        let commit = self
            .repin(lock, mount, pin.target, pin.blob, &canonical, &message)?
            .commit;
        let blob = mounted.repository().blob_id(&prepared.canonical);
        let location = self.repositories().snapshot().ok();
        let location = location.and_then(|mut view| view.resolve(path).ok());
        let location = location.unwrap_or_else(|| Location {
            board: board_model::Board {
                repository: inside.repository.clone(),
                source: Source::Commit(new),
            },
            path: inside.file.clone(),
            object: Object::Blob(blob),
            pointers: Vec::new(),
        });
        // References in the record are read in the mounted commit the root
        // now pins.
        let repositories = mounted.repositories();
        let resolve =
            &mut |path: &LogicalPath| repositories.resolve_in(repositories.root(), new, path);
        let degraded = mounted.degraded(resolve, &prepared, &inside.path);
        Ok(PutReport {
            writes: vec![Written {
                display: location.display_path().unwrap_or_else(|| path.clone()),
                location,
                previous: pinned,
                blob,
                degraded,
            }],
            commit,
        })
    }

    /// In this board, mounted at `mount`, puts `bytes` at `inside` under its
    /// own lock: when HEAD is on the branch `follow` names and descends from
    /// the pin, and the record file is the same at the pin and at HEAD, and
    /// in the working tree unless it already holds the record. Returns what
    /// it put, the pinned blob and the commit the record is in.
    fn put_mounted(
        &self,
        path: &LogicalPath,
        (mount, inside): (&LogicalPath, &Inside),
        follow: &str,
        expect: Expect,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<(Prepared, Option<ObjectId>, ObjectId), PutError> {
        let refuse = |why| refused(path, mount, why);
        if !self.config().commit_on_put {
            return Err(refuse(Through::Uncommitted));
        }
        let lock = self.repository().lock(wait)?;
        let head = self
            .repository()
            .writable_head(follow, inside.commit)
            .map_err(WriteError::from)?
            .map_err(refuse)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let schemas = &mut Schemas::default();
        let prepared = self
            .prepare(&lock, &mut view, schemas, &inside.path, Expect::Any, bytes)
            .map_err(|error| match error {
                PutError::Write(WriteError::ThroughMount { .. }) => refuse(Through::Nested),
                PutError::Record { refused, .. } => PutError::Record {
                    path: path.clone(),
                    refused,
                },
                error => error,
            })?;
        let [pinned, at_head, working] = self
            .repository()
            .held(inside.commit, head, &inside.file)
            .map_err(WriteError::from)?;
        let found = match expect {
            Expect::Any => true,
            Expect::Absent => pinned.is_none(),
            Expect::Blob(blob) => pinned == Some(blob),
        };
        if !found {
            return Err(PutError::Stale {
                path: path.clone(),
                found: pinned,
            });
        }
        let blob = Some(self.repository().blob_id(&prepared.canonical));
        // A put killed after its mounted commit left only the pin to move.
        let committed = at_head == blob && working == blob;
        let unchanged = pinned == at_head && (working == at_head || working == blob);
        // A link in the working tree may lead elsewhere than at the pin.
        if prepared.target.path != inside.file || !(committed || unchanged) {
            return Err(refuse(Through::Moved {
                file: inside.file.clone(),
            }));
        }
        if committed {
            return Ok((prepared, pinned, head));
        }
        let message = format!("{BINARY_NAME} put {}", inside.path);
        let written = self
            .write_all(lock, std::slice::from_ref(&prepared), &message)
            .map_err(|failure| match failure {
                Failure::Stale(mut stale) => stale.remove(0).1,
                Failure::Error(error) => error,
            })?;
        Ok((prepared, pinned, written.commit.unwrap_or(head)))
    }
}

/// A put to `path` through the mount at `mount` refused for `why`.
fn refused(path: &LogicalPath, mount: &LogicalPath, why: Through) -> PutError {
    PutError::Write(WriteError::ThroughMount {
        path: path.clone(),
        mount: mount.clone(),
        why: Box::new(why),
        inside: None,
    })
}

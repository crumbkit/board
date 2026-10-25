//! A put through one mount: an ordinary put in the mounted board, then the
//! mount's pin moved to the commit it made (docs/architecture/writes.md).

use std::collections::HashMap;
use std::time::Duration;

use board_model::{BINARY_NAME, Location, LogicalPath, Object, ObjectId, Source, repinned};
use board_tree::{Inside, Lock, PinError, Through, WriteError};

use super::batch::{message, refused_lines, shown_as};
use super::{Checked, Expect, Failure, Prepared, PutError, PutReport, ReadLine, Schemas, Written};
use crate::board::Board;

/// A line that leads through a mount: its number, its path from the root,
/// the mount it passes and where it lands in the mounted board.
pub(crate) struct Crossing {
    pub(crate) number: usize,
    pub(crate) path: LogicalPath,
    pub(crate) mount: LogicalPath,
    pub(crate) inside: Inside,
}

/// What a put through a mount writes: one record, or the lines of a batch
/// as the mounted board reads them.
pub(crate) enum Sent<'a> {
    Record(Expect, &'a [u8]),
    Batch(Vec<ReadLine>),
}

/// What the mounted board wrote: each item with the line it came from and
/// the blob at the pin, the commit they are in and the commit message.
struct Mounted<'a> {
    prepared: Vec<Prepared>,
    crossed: Vec<&'a Crossing>,
    pinned: Vec<Option<ObjectId>>,
    commit: ObjectId,
    message: String,
}

impl Board {
    /// Puts what `sent` holds through the mount every one of `crossings`
    /// passes, holding this board's `lock`, and waiting up to `wait` for
    /// the mounted board's. Each expect is compared with the pinned blob.
    pub(super) fn put_through(
        &self,
        lock: Lock,
        crossings: &[Crossing],
        sent: Sent<'_>,
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        let first = &crossings[0];
        let (mount, inside) = (&first.mount, &first.inside);
        if inside.repository == self.repository().path() {
            return Err(refused(&first.path, mount, Through::Own));
        }
        let home = self.repositories().home().map(Into::into);
        let mounted = match Board::open(&inside.repository, home) {
            Ok(mounted) if mounted.repository().path() == inside.repository => mounted,
            _ => return Err(refused(&first.path, mount, Through::NotABoard)),
        };
        // Refuses a `follow` that names nothing, as `mount update` does.
        let pin = self.repositories().pin(mount)?;
        let root = match sent {
            Sent::Record(..) => Some(format!("{BINARY_NAME} put {}", first.path)),
            Sent::Batch(_) => None,
        };
        let written = mounted.put_mounted(crossings, &pin.follow, sent, wait)?;
        let new = written.commit;
        // Repinned even when the pointer already names `new`, so a pin moved
        // by a put killed before the root committed it is committed now.
        let canonical = repinned(&pin.canonical, new).map_err(|error| PinError::Mount {
            path: mount.clone(),
            code: error.code(),
        })?;
        let message = root.unwrap_or(written.message);
        let commit = self
            .repin(lock, mount, pin.target, pin.blob, &canonical, &message)?
            .commit;
        let mut view = self.repositories().snapshot().ok();
        // References in each record are read in the mounted commit the root
        // now pins.
        let repositories = mounted.repositories();
        let resolve =
            &mut |path: &LogicalPath| repositories.resolve_in(repositories.root(), new, path);
        let root = &mut repositories.root().reader();
        let mut writes = Vec::with_capacity(written.prepared.len());
        let items = written.prepared.iter().zip(written.crossed);
        for ((item, crossing), previous) in items.zip(written.pinned) {
            let blob = mounted.repository().blob_id(&item.canonical);
            let location = view
                .as_mut()
                .and_then(|view| view.resolve(&crossing.path).ok());
            let location = location.unwrap_or_else(|| Location {
                board: board_model::Board {
                    repository: inside.repository.clone(),
                    source: Source::Commit(new),
                },
                path: crossing.inside.file.clone(),
                object: Object::Blob(blob),
                pointers: Vec::new(),
            });
            writes.push(Written {
                display: location
                    .display_path()
                    .unwrap_or_else(|| crossing.path.clone()),
                location,
                previous,
                blob,
                degraded: mounted.degraded((resolve, root), item, &crossing.inside.path),
            });
        }
        Ok(PutReport { writes, commit })
    }

    /// In this board, mounted where `crossings` lead, puts what `sent`
    /// holds under its own lock: when HEAD is on the branch `follow` names
    /// and descends from the pin, and each file is the same at the pin and
    /// at HEAD, and in the working tree unless it already holds what is put.
    fn put_mounted<'a>(
        &self,
        crossings: &'a [Crossing],
        follow: &str,
        sent: Sent<'_>,
        wait: Duration,
    ) -> Result<Mounted<'a>, PutError> {
        let first = &crossings[0];
        let refuse = |path: &LogicalPath, why| refused(path, &first.mount, why);
        if !self.config().commit_on_put {
            return Err(refuse(&first.path, Through::Uncommitted));
        }
        let lock = self.repository().lock(wait)?;
        let head = self
            .repository()
            .writable_head(follow, first.inside.commit)
            .map_err(WriteError::from)?
            .map_err(|why| refuse(&first.path, why))?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let batch = matches!(sent, Sent::Batch(_));
        let (message, mut prepared, crossed) = match sent {
            Sent::Record(expect, bytes) => {
                let schemas = &mut Schemas::default();
                let item = self
                    .prepare(&lock, &mut view, schemas, &first.inside.path, expect, bytes)
                    .map_err(|error| match error {
                        PutError::Write(WriteError::ThroughMount { .. }) => {
                            refuse(&first.path, Through::Nested)
                        }
                        error => shown_as(error, &first.path),
                    })?;
                let message = format!("{BINARY_NAME} put {}", first.inside.path);
                (message, vec![item], vec![first])
            }
            Sent::Batch(lines) => {
                match self.check_lines(&lock, &mut view, lines, &mut Schemas::default())? {
                    Checked::Here(prepared, numbers) => {
                        let by_number: HashMap<usize, &Crossing> = crossings
                            .iter()
                            .map(|crossing| (crossing.number, crossing))
                            .collect();
                        let crossed = numbers.iter().map(|number| by_number[number]).collect();
                        (message(&prepared), prepared, crossed)
                    }
                    Checked::Through(..) => return Err(refuse(&first.path, Through::Nested)),
                }
            }
        };
        let (mut pinned, mut failures, mut committed) = (Vec::new(), Vec::new(), true);
        for (item, crossing) in prepared.iter_mut().zip(&crossed) {
            let (number, path, inside) = (crossing.number, &crossing.path, &crossing.inside);
            let [at_pin, at_head, working] = self
                .repository()
                .held(inside.commit, head, &inside.file)
                .map_err(WriteError::from)?;
            let found = match std::mem::replace(&mut item.expect, Expect::Any) {
                Expect::Any => true,
                Expect::Absent => at_pin.is_none(),
                Expect::Blob(blob) => at_pin == Some(blob),
            };
            let blob = Some(self.repository().blob_id(&item.canonical));
            // A put killed after its mounted commit left only the pin to move.
            let done = at_head == blob && working == blob;
            let unchanged = at_pin == at_head && (working == at_head || working == blob);
            if !found {
                let found = at_pin;
                failures.push((
                    number,
                    PutError::Stale {
                        path: path.clone(),
                        found,
                    },
                ));
            // A link in the working tree may lead elsewhere than at the pin.
            } else if item.target.path != inside.file || !(done || unchanged) {
                let file = inside.file.clone();
                failures.push((number, refuse(path, Through::Moved { file })));
            }
            committed &= done;
            pinned.push(at_pin);
        }
        if !failures.is_empty() {
            return Err(if batch {
                refused_lines(&failures)
            } else {
                failures.remove(0).1
            });
        }
        let commit = if committed {
            head
        } else {
            self.write_all(lock, &prepared, &[], &message)
                .map_err(Failure::one)?
                .commit
                .unwrap_or(head)
        };
        Ok(Mounted {
            prepared,
            crossed,
            pinned,
            commit,
            message,
        })
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

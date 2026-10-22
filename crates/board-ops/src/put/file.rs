//! `board put --file`: writing a file as given, or, past the inline
//! limit with a store, its attachment record (docs/architecture/attachments.md).

use std::time::Duration;

use board_model::{BINARY_NAME, LogicalPath, MAX_RECORD_BYTES, Role};
use board_tree::{LOCK_WAIT, Lock, Resolver, WriteError};
use board_validate::Reference;
use serde_json::Value;

use super::{Expect, Failure, Prepared, PutError, PutReport};
use crate::board::Board;
use crate::verdict::Admitted;

impl Board {
    /// Takes the lock, waiting up to [`LOCK_WAIT`], and writes `bytes` as
    /// given to the file whose whole name is `path`, only if the file is
    /// what `expect` says.
    pub fn put_file(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<PutReport, PutError> {
        self.put_file_waiting(path, expect, bytes, LOCK_WAIT)
    }

    /// [`Board::put_file`], waiting up to `wait` for the lock.
    pub fn put_file_waiting(
        &self,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        if let Some(record) = self.attach(path, bytes)? {
            return self.put_waiting(path, expect, &record, wait);
        }
        let lock = self.repository().lock(wait)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let prepared = Self::prepare_file(&lock, &mut view, path, expect, bytes)?;
        let message = format!("{BINARY_NAME} put {path}");
        self.write_all(lock, &[prepared], &message)
            .map_err(Failure::one)
    }

    /// Finds where the file whose whole name is `path` lands in `view`, for
    /// `bytes` written as given, which need only fit the size limit, under
    /// the lock that the write will hold. A Markdown file's links are its
    /// references, so the report says what `check` will.
    pub(super) fn prepare_file(
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        path: &LogicalPath,
        expect: Expect,
        bytes: &[u8],
    ) -> Result<Prepared, PutError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(PutError::TooLarge {
                path: path.clone(),
                bytes: bytes.len(),
            });
        }
        let target = view.file_target(path)?;
        // Read from where the file lands, as `check` reads it, and not from
        // a path through a link.
        let references = match (Role::of(&target.path), format!("/{}", target.path).parse()) {
            (Some(Role::Markdown), Ok(landed)) => references(&landed, bytes),
            _ => Vec::new(),
        };
        Ok(Prepared {
            path: path.clone(),
            expect,
            canonical: bytes.to_vec(),
            target,
            admitted: Admitted {
                value: Value::Null,
                pointer: false,
                references,
            },
        })
    }
}

/// The links in `bytes`, a Markdown file at `path`, as references, so a
/// link to nothing degrades the file in the report as `Decide::links` does
/// in `check`.
fn references(path: &LogicalPath, bytes: &[u8]) -> Vec<Reference> {
    board_model::links(bytes)
        .into_iter()
        .filter_map(|link| {
            Some(Reference {
                path: board_model::reached(path, &link)?,
                pointer: link,
                accepts: None,
            })
        })
        .collect()
}

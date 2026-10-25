//! `board put --file`: writing a file as given, or, past the inline
//! limit with a store, its attachment record (docs/architecture/attachments.md).

use std::time::Duration;

use board_model::{BINARY_NAME, Classified, LogicalPath, MAX_RECORD_BYTES, Role, frontmatter};
use board_tree::{LOCK_WAIT, Lock, Resolver, WriteError};
use board_validate::Reference;
use serde_json::Value;

use super::{Expect, Failure, Prepared, PutError, PutReport, Refused, Schemas};
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
        let schemas = &mut Schemas::default();
        let prepared = self.prepare_file(&lock, &mut view, schemas, path, expect, bytes)?;
        let message = format!("{BINARY_NAME} put {path}");
        self.write_all(lock, &[prepared], &[], &message)
            .map_err(Failure::one)
    }

    /// Finds where the file whose whole name is `path` lands in `view`, for
    /// `bytes` written as given, under the lock that the write will hold.
    /// They need only fit the size limit, unless they are a Markdown file
    /// with frontmatter that claims a `$type`, which must parse and be a
    /// valid record. A Markdown file's links are its references, after
    /// any its frontmatter makes, so the report says what `check` will.
    pub(super) fn prepare_file<'b>(
        &'b self,
        _lock: &Lock,
        view: &mut Resolver<'_, '_>,
        schemas: &mut Schemas<'b>,
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
        let mut admitted = Admitted {
            value: Value::Null,
            pointer: false,
            references: Vec::new(),
        };
        if Role::of(&target.path) == Some(Role::Markdown) {
            let refused = |refused| PutError::Record {
                path: path.clone(),
                refused,
            };
            if let Some(record) = self.markdown_record(schemas, bytes) {
                admitted = record.map_err(refused)?;
            }
            if let Ok(landed) = format!("/{}", target.path).parse() {
                admitted.references.extend(references(&landed, bytes));
            }
        }
        Ok(Prepared {
            path: path.clone(),
            expect,
            canonical: bytes.to_vec(),
            target,
            admitted,
        })
    }

    /// Checks the frontmatter of a Markdown file as `check` does, and the
    /// record it holds against its schema. `None` for a file with no
    /// frontmatter that claims a `$type`.
    fn markdown_record<'b>(
        &'b self,
        schemas: &mut Schemas<'b>,
        bytes: &[u8],
    ) -> Option<Result<Admitted, Refused>> {
        let classified = match frontmatter(bytes) {
            Ok(None) => return None,
            Err(error) => return Some(Err(error.into())),
            Ok(Some(value)) => Classified::frontmatter(value),
        };
        Some(self.admit(schemas, classified).map_err(Refused::from))
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

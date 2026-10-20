//! Attachments: a put of a large or binary file uploads the bytes to the
//! board's store under their digest and writes a `board/attachment/v1`
//! record in their place; `get --fetch` reads them back and checks the
//! digest (docs/architecture/attachments.md).

use std::io::ErrorKind;

use board_model::{
    ATTACHMENT_TYPE, Attachment, Code, Digest, LogicalPath, canonical_json, media_type,
};
use serde_json::Value;

use crate::board::{Backend, Board};
use crate::put::PutError;
use crate::read::{GetReport, ReadError};

/// The largest file a put with a store writes into the tree. A larger
/// file, or one that is not UTF-8, goes to the store.
pub const MAX_INLINE_BYTES: usize = 1024 * 1024;

impl Board {
    /// The record for `bytes` put with `--file` at `path`, once they are
    /// uploaded to the store: `None` when the board has no store, or when
    /// the bytes are UTF-8 and small enough for the tree.
    pub(crate) fn attach(
        &self,
        path: &LogicalPath,
        bytes: &[u8],
    ) -> Result<Option<Vec<u8>>, PutError> {
        let Some(store) = &self.config().store else {
            return Ok(None);
        };
        if bytes.len() <= MAX_INLINE_BYTES && std::str::from_utf8(bytes).is_ok() {
            return Ok(None);
        }
        let digest = Digest::of(bytes);
        let location = match store.backend {
            Backend::Directory => {
                let directory = self.repository().path().join(&store.location);
                board_tree::write_outside(&directory, &digest.hex(), bytes)
                    .map_err(|source| PutError::Upload {
                        path: path.clone(),
                        location: directory.display().to_string(),
                        source,
                    })?
                    .display()
                    .to_string()
            }
        };
        let name = path.segments().last().unwrap_or_default();
        let size = bytes.len() as u64;
        let attachment = Attachment::new(digest, size, media_type(name).to_owned(), location);
        let value = serde_json::to_value(attachment).map_err(|error| PutError::Line {
            code: Code::PutFailed,
            reason: error.to_string(),
        })?;
        Ok(Some(canonical_json(&value).into_bytes()))
    }

    /// `report` with the bytes its attachment record names fetched from
    /// the store and checked against the record's digest; any other file
    /// as it is.
    pub fn fetched(&self, report: GetReport) -> Result<GetReport, ReadError> {
        let Ok(value) = &report.record else {
            return Ok(report);
        };
        if value.get("$type").and_then(Value::as_str) != Some(ATTACHMENT_TYPE) {
            return Ok(report);
        }
        let fetched = Attachment::from_value(value)
            .map_err(|source| FetchError::Invalid { source })
            .and_then(|attachment| fetch(&attachment));
        match fetched {
            Ok(bytes) => Ok(GetReport { bytes, ..report }),
            Err(source) => Err(ReadError::Fetch {
                path: report.display,
                source,
            }),
        }
    }
}

/// The bytes at `attachment`'s location, which must hash to its digest.
fn fetch(attachment: &Attachment) -> Result<Vec<u8>, FetchError> {
    let location = attachment.location.clone();
    let bytes = match std::fs::read(&location) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(FetchError::Missing { location });
        }
        Err(source) => return Err(FetchError::Unreadable { location, source }),
    };
    let found = Digest::of(&bytes);
    if found != attachment.digest {
        return Err(FetchError::Mismatch {
            location,
            expected: attachment.digest,
            found,
        });
    }
    Ok(bytes)
}

/// Why an attachment's bytes could not be fetched.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("is an attachment record that does not read as one: {source}")]
    Invalid { source: serde_json::Error },
    #[error("names {location}, which holds nothing")]
    Missing { location: String },
    #[error("names {location}, which cannot be read: {source}")]
    Unreadable {
        location: String,
        source: std::io::Error,
    },
    #[error("names {location}, whose bytes hash to {found}, not the record's {expected}")]
    Mismatch {
        location: String,
        expected: Digest,
        found: Digest,
    },
}

impl FetchError {
    pub fn code(&self) -> Code {
        match self {
            Self::Invalid { .. } => Code::RecordInvalid,
            Self::Missing { .. } => Code::AttachmentMissing,
            Self::Unreadable { .. } => Code::RepoUnreadable,
            Self::Mismatch { .. } => Code::AttachmentMismatch,
        }
    }
}

#[cfg(test)]
mod tests;

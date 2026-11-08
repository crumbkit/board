//! The `Board-Bulk:` trailer that marks a commit touching many files
//! (docs/architecture/paths/writes.md, Bulk commits).

use super::PutError;

/// Why a commit touches many files at once, such as `migrate flows`: one
/// line of 1 to 80 characters, which the commit gives in a
/// `Board-Bulk: <reason>` trailer as the last line of its message, so a
/// reader of history can tell a bulk commit from an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bulk(String);

impl Bulk {
    /// The most characters a reason holds.
    pub const MAX_CHARS: usize = 80;
    /// The trailer's key.
    pub const TRAILER: &'static str = "Board-Bulk";

    /// The reason `reason`, refused as `board::put::bulk_reason` when it is
    /// empty, holds a line break or is over 80 characters.
    pub fn new(reason: &str) -> Result<Self, PutError> {
        let empty = reason.trim().is_empty();
        let broken = reason.contains(['\n', '\r']);
        if empty || broken || reason.chars().count() > Self::MAX_CHARS {
            return Err(PutError::BulkReason {
                reason: reason.to_owned(),
            });
        }
        Ok(Self(reason.to_owned()))
    }

    /// `message` with this trailer as its last line, unless its last line
    /// already is a `Board-Bulk:` trailer.
    pub fn mark(&self, message: &str) -> String {
        let prefix = format!("{}: ", Self::TRAILER);
        if message
            .lines()
            .last()
            .is_some_and(|line| line.starts_with(&prefix))
        {
            return message.to_owned();
        }
        format!("{message}\n\n{prefix}{}", self.0)
    }
}

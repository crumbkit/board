//! What the notes, tasks and prompts plugins share: notes repos and the
//! git remotes their records cover, reading and writing a repo's files,
//! the write-and-retry loop, Reflect's slug and the person's day. Each
//! function takes the calling plugin's name for its codes.

mod day;
#[cfg(test)]
mod fake;
mod listing;
mod repo;
mod slug;
mod writing;

use board_ops::PluginError;
use serde_json::Value;

pub use day::{Date, day_start, today};
pub use listing::{
    COLUMNS, Listed, RECORD, RECORD_TYPE, covering, covers, json_body, listed, parsed, query,
    record, root, scope, shown,
};
pub use repo::{File, Repo};
pub use slug::slug;
pub use writing::{
    ATTEMPTS, Clones, EDITED, MOVED, Through, Writer, create, landed, push, refused,
};

/// Text without trailing whitespace, as Python's `str.rstrip` drops it,
/// which also counts U+001C to U+001F.
pub fn trimmed(text: &str) -> &str {
    text.trim_end_matches(python_space)
}

/// Whether Python's `str.isspace` holds for `c`.
pub fn python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// A refusal of the calling plugin's own, with `fields`.
pub(crate) fn refusal<const N: usize>(
    code: String,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code,
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        hint: None,
    }
}

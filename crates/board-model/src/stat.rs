//! What the file system says about a working-tree file, and whether it is
//! what `HEAD` holds (docs/architecture.md, The index).

/// A file's metadata, as git compares it to decide whether a file must be
/// hashed again. Times are nanoseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub modified_ns: i64,
    pub changed_ns: i64,
    pub inode: u64,
    pub mode: u32,
}

/// Whether a working-tree entry is what `HEAD` holds at its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Committed,
    Uncommitted,
}

impl Change {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Uncommitted => "uncommitted",
        }
    }
}

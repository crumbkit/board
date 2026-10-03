//! What the file system says about a working-tree file (docs/architecture.md,
//! The index).

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

//! Marks the cache root, `<cache>/board`, as a cache, so backups leave it
//! out (docs/architecture/performance/timing.md). The tag's presence means
//! marked, so a marked root costs one stat.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

use board_model::Code;

/// A cache directory tag, as <https://bford.info/cachedir/> sets it out.
const TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by Board.\n\
# For information about cache directory tags see https://bford.info/cachedir/\n";

/// The binary plist of the string `com.apple.backupd`, the bytes
/// `tmutil addexclusion` writes, read back with `xattr -px`.
#[cfg(target_os = "macos")]
const EXCLUDED: &[u8] = b"bplist00_\x10\x11com.apple.backupd\x08\
\0\0\0\0\0\0\x01\x01\0\0\0\0\0\0\0\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x1c";

/// A cache root that could not be marked.
#[derive(Debug, thiserror::Error)]
#[error(
    "the cache directory {} is not marked as a cache, so backups may copy it: {error}",
    directory.display()
)]
pub struct Unmarked {
    directory: PathBuf,
    error: io::Error,
}

impl Unmarked {
    pub fn code(&self) -> Code {
        Code::CacheUnmarked
    }
}

std::thread_local! {
    /// The roots this thread could not mark, and each it warned about.
    static UNMARKED: RefCell<(Vec<Unmarked>, Vec<PathBuf>)> = const {
        RefCell::new((Vec::new(), Vec::new()))
    };
}

/// Marks `root` unless its tag is there. A failure never fails the caller;
/// [`take_unmarked`] gives it, once for each root.
pub(crate) fn mark(root: &Path) {
    let Err(error) = marked(root) else {
        return;
    };
    UNMARKED.with(|unmarked| {
        let (pending, warned) = &mut *unmarked.borrow_mut();
        if !warned.iter().any(|directory| directory == root) {
            warned.push(root.to_path_buf());
            pending.push(Unmarked {
                directory: root.to_path_buf(),
                error,
            });
        }
    });
}

/// The Time Machine exclusion goes first: the tag stops later tries.
fn marked(root: &Path) -> io::Result<()> {
    let tag = root.join("CACHEDIR.TAG");
    if tag.try_exists()? {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    rustix::fs::setxattr(
        root,
        "com.apple.metadata:com_apple_backup_excludeItem",
        EXCLUDED,
        rustix::fs::XattrFlags::empty(),
    )?;
    crate::state::replace_file(&tag, TAG, 0o644)
}

/// Takes the cache roots this thread could not mark since the last call.
pub fn take_unmarked() -> Vec<Unmarked> {
    UNMARKED.with(|unmarked| std::mem::take(&mut unmarked.borrow_mut().0))
}

#[cfg(test)]
mod tests;

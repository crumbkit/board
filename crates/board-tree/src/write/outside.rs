//! Writes outside any working tree, such as a directory-backed attachment
//! store (docs/architecture/attachments.md).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::write_temporary;

/// Writes `bytes` to the file `name` in `directory`, which need not be in
/// any working tree, as [`crate::Repository::write_file`] writes: through a
/// temporary file beside it and a rename, so a kill leaves the old file or
/// the new one. Returns the file's path.
pub fn write_outside(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let target = directory.join(name);
    let temporary = directory.join(format!(".{name}.board-tmp"));
    let written =
        write_temporary(&temporary, &target, bytes).and_then(|()| fs::rename(&temporary, &target));
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(target)
}

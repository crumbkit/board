//! Files a built-in plugin keeps on this machine outside every board, such
//! as autosync's status, log, run lock and launchd agent. board-index owns
//! them as it owns the cache's files (STANDARDS.md 7).

use std::io::{self, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::Instant;

use crate::server::{private_directory, write_private};

/// Writes `bytes` at `file`, readable only by its owner, in a directory only
/// its owner can enter, renamed into place so a reader never sees it half
/// written.
pub fn write_state(file: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private(file, bytes)
}

/// Writes `bytes` at `file` with `mode`, through a temporary file in the
/// same directory renamed into place, and leaves the directory's own mode
/// as it is, as a file in a shared directory such as `~/Library/LaunchAgents`
/// needs. A crash before the rename leaves the old file whole.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the files a plugin keeps outside every board"
)]
pub fn replace_file(file: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let directory = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    temporary.write_all(bytes)?;
    temporary.persist(file).map_err(|error| error.error)?;
    Ok(())
}

/// Appends `line` and a newline to `file`, created readable only by its
/// owner in a directory only its owner can enter. When the file already
/// holds more than `rotate_above` bytes, it is first renamed to
/// `<file>.1`, replacing an older one.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the files a plugin keeps outside every board"
)]
pub fn append_state(file: &Path, line: &str, rotate_above: u64) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    private_directory(file.parent().unwrap_or(Path::new(".")))?;
    if std::fs::metadata(file).is_ok_and(|held| held.len() > rotate_above) {
        let mut rotated = file.as_os_str().to_owned();
        rotated.push(".1");
        std::fs::rename(file, rotated)?;
    }
    let mut opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file)?;
    opened.write_all(format!("{line}\n").as_bytes())
}

/// Removes `file`; one already gone is not an error.
pub fn remove_state(file: &Path) -> io::Result<()> {
    crate::remove_opener(file)
}

/// A new directory whose name starts with `prefix` in `parent`, made only
/// its owner can enter, removed with what it holds when the value drops.
pub fn scratch_directory(parent: &Path, prefix: &str) -> io::Result<tempfile::TempDir> {
    private_directory(parent)?;
    tempfile::Builder::new()
        .prefix(prefix)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
}

/// Removes every directory in `parent` whose name starts with `prefix`,
/// such as one a killed run left. A missing `parent` holds none.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the files a plugin keeps outside every board"
)]
pub fn remove_scratch(parent: &Path, prefix: &str) -> io::Result<()> {
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let named = entry.file_name().to_string_lossy().starts_with(prefix);
        if named && entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// An exclusive lock on `file`, created readable only by its owner in a
/// directory only its owner can enter, or `None` at once when another
/// process, or another holder in this one, has it. The kernel releases it
/// when the process ends, however it ends.
pub fn lock_state(file: &Path) -> io::Result<Option<crumbkit_durable::PathLock>> {
    use crumbkit_durable::{LockError, PathLock};
    private_directory(file.parent().unwrap_or(Path::new(".")))?;
    match PathLock::acquire(file, Instant::now()) {
        Ok(lock) => Ok(Some(lock)),
        Err(LockError::Io { source, .. }) => Err(source),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests;

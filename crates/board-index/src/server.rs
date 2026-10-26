//! The files in a board's cache that `board serve` and `board ui open`
//! share (docs/architecture/ui/serve.md): the server file, which names the
//! running server, and the openers, pages that send the browser to it.
//! `board answer` writes one more: the statement an approval signs.
//! This crate stores the bytes it is given; `board-serve` decides what they
//! say. Both files hold the server's token, so the board's cache directory
//! is its owner's alone and each file is readable only by its owner.

use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::open::board;

/// Where the server file of the board at `root` is, under `cache`.
pub fn server_file(cache: &Path, root: &Path) -> PathBuf {
    board(cache, root).join("server.json")
}

/// The user this process runs as. A new pipe belongs to the user that made
/// it, so its owner is this process's user.
fn user() -> io::Result<u32> {
    let (reader, _writer) = io::pipe()?;
    Ok(std::fs::File::from(std::os::fd::OwnedFd::from(reader))
        .metadata()?
        .uid())
}

/// Writes `bytes` as the server file.
pub fn write_server(cache: &Path, root: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private(&server_file(cache, root), bytes)
}

/// Writes `bytes` as a new opener, `open-<random>.html`, readable only by
/// its owner, and returns its path. Openers older than `stale` are removed.
pub fn write_opener(
    cache: &Path,
    root: &Path,
    bytes: &[u8],
    stale: Duration,
) -> io::Result<PathBuf> {
    let directory = board(cache, root);
    let directory = private_directory(&directory)?;
    for entry in std::fs::read_dir(directory)?.flatten() {
        let name = entry.file_name();
        let opener = name.to_str().is_some_and(|name| name.starts_with("open-"));
        let age = entry.metadata().and_then(|metadata| metadata.modified());
        if opener && age.is_ok_and(|time| time.elapsed().is_ok_and(|age| age > stale)) {
            // An opener that cannot be removed now is tried again next time.
            let _ = remove_opener(&entry.path());
        }
    }
    let mut file = tempfile::Builder::new()
        .prefix("open-")
        .suffix(".html")
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(directory)?;
    file.write_all(bytes)?;
    file.keep()
        .map(|(_, path)| path)
        .map_err(|error| error.error)
}

/// Writes `bytes` as a statement for a signer to read, `approve-<random>.txt`
/// in the cache directory of the board at `root`, readable only by its
/// owner. The file is removed when the returned path drops.
pub fn write_statement(cache: &Path, root: &Path, bytes: &[u8]) -> io::Result<tempfile::TempPath> {
    let directory = board(cache, root);
    let directory = private_directory(&directory)?;
    let mut file = tempfile::Builder::new()
        .prefix("approve-")
        .suffix(".txt")
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(directory)?;
    file.write_all(bytes)?;
    Ok(file.into_temp_path())
}

/// Removes the opener at `file`; one already gone is not an error.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn remove_opener(file: &Path) -> io::Result<()> {
    match std::fs::remove_file(file) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Makes `directory` and leaves it enterable only by its owner.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
fn private_directory(directory: &Path) -> io::Result<&Path> {
    std::fs::create_dir_all(directory)?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}

/// Writes `bytes` at `file`, readable only by its owner, in a directory only
/// its owner can enter, renamed into place so a reader never sees it half
/// written.
fn write_private(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = private_directory(file.parent().unwrap_or(Path::new(".")))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    temporary.write_all(bytes)?;
    temporary.persist(file).map_err(|error| error.error)?;
    Ok(())
}

/// The server file's bytes, or `None` when there is none. The file must be
/// a regular file of this user with mode 0600 and one link, in a directory
/// of this user with mode 0700. Any other could have been planted by someone
/// else, so it is refused. The file is checked with lstat before it is
/// opened, so a link or a FIFO is never opened; the open file must then have
/// the lstat's device and inode, which catches a link swapped in after the
/// lstat.
pub fn read_server(cache: &Path, root: &Path) -> io::Result<Option<Vec<u8>>> {
    let path = server_file(cache, root);
    let untrusted = |what: &str| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {what}", path.display()),
        )
    };
    let user = user()?;
    let directory = match std::fs::symlink_metadata(path.parent().unwrap_or(Path::new("."))) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !directory.is_dir() || directory.uid() != user || directory.mode() & 0o777 != 0o700 {
        return Err(untrusted("is not in a directory of yours with mode 0700"));
    }
    let link = match std::fs::symlink_metadata(&path) {
        Ok(link) => link,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if link.file_type().is_symlink() {
        return Err(untrusted("is a link"));
    }
    let private = |metadata: &std::fs::Metadata| {
        metadata.is_file()
            && metadata.uid() == user
            && metadata.mode() & 0o777 == 0o600
            && metadata.nlink() == 1
    };
    let refused = "is not a regular file of yours with mode 0600 and one link";
    if !private(&link) {
        return Err(untrusted(refused));
    }
    let mut file = std::fs::File::open(&path)?;
    let metadata = file.metadata()?;
    if metadata.dev() != link.dev() || metadata.ino() != link.ino() {
        return Err(untrusted("was replaced while it was opened"));
    }
    if !private(&metadata) {
        return Err(untrusted(refused));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Removes the server file only while it still holds `bytes`, so a server
/// that exits never removes the file of the server that replaced it.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn remove_server(cache: &Path, root: &Path, bytes: &[u8]) -> io::Result<()> {
    if read_server(cache, root)?.as_deref() != Some(bytes) {
        return Ok(());
    }
    match std::fs::remove_file(server_file(cache, root)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_server_file_and_opener_are_written_for_their_owner_alone_and_read_back() {
        let cache = tempfile::tempdir().unwrap();
        let root = Path::new("/boards/a");
        assert_eq!(read_server(cache.path(), root).unwrap(), None);
        write_server(cache.path(), root, b"one").unwrap();
        assert_eq!(
            read_server(cache.path(), root).unwrap().as_deref(),
            Some(&b"one"[..])
        );
        let opener = write_opener(cache.path(), root, b"<p>", Duration::from_mins(1)).unwrap();
        assert_eq!(std::fs::read(&opener).unwrap(), b"<p>");
        assert_eq!(mode(&server_file(cache.path(), root)), 0o600);
        assert_eq!(mode(&opener), 0o600);
        assert_eq!(mode(opener.parent().unwrap()), 0o700);
    }

    #[test]
    fn a_server_file_others_could_have_written_is_refused_and_one_of_mode_0600_is_read() {
        let cache = tempfile::tempdir().unwrap();
        let (root, other) = (Path::new("/boards/a"), Path::new("/boards/b"));
        write_server(cache.path(), root, b"one").unwrap();
        let file = server_file(cache.path(), root);

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let refused = read_server(cache.path(), root).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_server(cache.path(), root).unwrap().is_some());

        // A link to a good server file is still refused.
        write_opener(cache.path(), other, b"<p>", Duration::ZERO).unwrap();
        std::os::unix::fs::symlink(&file, server_file(cache.path(), other)).unwrap();
        let refused = read_server(cache.path(), other).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test moves a socket to where the server file goes"
    )]
    fn a_server_file_that_is_not_a_regular_file_is_refused_unopened_and_a_regular_one_is_read() {
        let cache = tempfile::tempdir().unwrap();
        let root = Path::new("/boards/a");
        write_server(cache.path(), root, b"one").unwrap();
        let file = server_file(cache.path(), root);

        // A socket stands in for a FIFO: opening either is never safe, and
        // std makes a socket without a child process or unsafe code. A
        // socket's path has a short length limit, so it is moved into place.
        let short = cache.path().join("s");
        let socket = std::os::unix::net::UnixListener::bind(&short).unwrap();
        std::fs::rename(&short, &file).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let refused = read_server(cache.path(), root).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            refused
                .to_string()
                .ends_with(" is not a regular file of yours with mode 0600 and one link")
        );
        drop(socket);

        write_server(cache.path(), root, b"one").unwrap();
        assert!(read_server(cache.path(), root).unwrap().is_some());
    }

    #[test]
    fn a_server_file_with_a_second_link_or_in_an_open_directory_is_refused_and_a_private_one_is_read()
     {
        let cache = tempfile::tempdir().unwrap();
        let root = Path::new("/boards/a");
        write_server(cache.path(), root, b"one").unwrap();
        let file = server_file(cache.path(), root);
        let directory = file.parent().unwrap();

        std::fs::hard_link(&file, cache.path().join("copy")).unwrap();
        let refused = read_server(cache.path(), root).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        write_server(cache.path(), root, b"one").unwrap();
        assert!(read_server(cache.path(), root).unwrap().is_some());

        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        let refused = read_server(cache.path(), root).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(read_server(cache.path(), root).unwrap().is_some());
    }

    #[test]
    fn each_opener_has_its_own_file_and_writing_one_removes_stale_openers_but_not_fresh_ones() {
        let cache = tempfile::tempdir().unwrap();
        let root = Path::new("/boards/a");
        let hour = Duration::from_hours(1);
        let first = write_opener(cache.path(), root, b"1", hour).unwrap();
        let second = write_opener(cache.path(), root, b"2", hour).unwrap();
        assert_ne!(first, second);
        assert!(first.exists() && second.exists());

        std::thread::sleep(Duration::from_millis(20));
        let third = write_opener(cache.path(), root, b"3", Duration::from_millis(10)).unwrap();
        assert!(!first.exists() && !second.exists());
        assert!(third.exists());
    }

    #[test]
    fn a_server_removes_only_its_own_file_and_not_its_replacements() {
        let cache = tempfile::tempdir().unwrap();
        let root = Path::new("/boards/a");
        write_server(cache.path(), root, b"two").unwrap();
        remove_server(cache.path(), root, b"one").unwrap();
        assert!(read_server(cache.path(), root).unwrap().is_some());
        remove_server(cache.path(), root, b"two").unwrap();
        assert_eq!(read_server(cache.path(), root).unwrap(), None);
    }
}

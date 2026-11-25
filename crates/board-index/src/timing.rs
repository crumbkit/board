//! The timing log, its digest and the speed check's stamp
//! (docs/architecture/performance/timing.md).

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::time::SystemTime;

/// The size past which the log is renamed to `timings.jsonl.1`.
const TIMING_LOG_BYTES: u64 = 1024 * 1024;

/// What rotation makes of a log and the old digest: the new digest.
pub type Summarise = dyn Fn(&[u8], &[u8]) -> Vec<u8>;

/// The log, the log it replaced and the digest of the logs before those,
/// each empty when absent.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Timings {
    pub digest: Vec<u8>,
    pub previous: Vec<u8>,
    pub current: Vec<u8>,
}

/// Appends `line` to the timing log under `cache`, which only its owner can
/// read. When `line` would take the log past 1 MiB, the run that takes the
/// rotation lock first replaces the digest with what `digest` makes of the
/// log and the old digest, then renames the log to `timings.jsonl.1`. A run
/// that finds the lock held appends without rotating. A digest that cannot
/// be written leaves the log's hours unsummarised and still rotates it.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn append_timing(cache: &Path, line: &[u8], digest: &Summarise) -> io::Result<()> {
    let directory = cache.join("board");
    std::fs::create_dir_all(&directory)?;
    crate::mark::mark(&directory);
    let log = directory.join("timings.jsonl");
    if length(&log).saturating_add(line.len() as u64) > TIMING_LOG_BYTES {
        rotate(&directory, &log, line.len() as u64, digest)?;
    }
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&log)?
        .write_all(line)
}

fn length(file: &Path) -> u64 {
    std::fs::metadata(file).map_or(0, |metadata| metadata.len())
}

/// Rotates `log` under the rotation lock, unless another run holds the lock
/// or rotated it first. The lock is the kernel's, so a killed run frees it.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
fn rotate(directory: &Path, log: &Path, adding: u64, digest: &Summarise) -> io::Result<()> {
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.join("timings.lock"))?;
    if lock.try_lock().is_err() || length(log).saturating_add(adding) <= TIMING_LOG_BYTES {
        return Ok(());
    }
    let summarised = directory.join("timing-digest.jsonl");
    let old = std::fs::read(&summarised).unwrap_or_default();
    if let Ok(bytes) = std::fs::read(log) {
        // A digest that cannot be written is a gap the check reports; the
        // log still rotates, so it never grows without bound.
        let _ = crate::state::replace_file(&summarised, &digest(&bytes, &old), 0o600);
    }
    std::fs::rename(log, log.with_extension("jsonl.1"))
}

/// The digest and both logs under `cache`.
pub fn read_timings(cache: &Path) -> Timings {
    let read = |name: &str| std::fs::read(cache.join("board").join(name)).unwrap_or_default();
    Timings {
        digest: read("timing-digest.jsonl"),
        previous: read("timings.jsonl.1"),
        current: read("timings.jsonl"),
    }
}

/// When the speed check last recorded a run, if it has.
pub fn timing_stamped(cache: &Path) -> Option<SystemTime> {
    std::fs::metadata(cache.join("board/timing-check.stamp"))
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Notes that the speed check recorded a run now.
pub fn stamp_timing(cache: &Path) -> io::Result<()> {
    crate::state::replace_file(&cache.join("board/timing-check.stamp"), b"", 0o600)
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the tests hold the rotation lock and put a directory where the digest belongs"
)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn lines(path: &Path) -> usize {
        std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
    }

    /// A digest that names how many bytes of log it saw after the old one.
    fn counted(log: &[u8], old: &[u8]) -> Vec<u8> {
        let mut digest = old.to_vec();
        digest.extend(format!("{}\n", log.len()).into_bytes());
        digest
    }

    #[test]
    fn a_line_is_appended_to_a_log_only_its_owner_can_read() {
        let cache = tempfile::tempdir().unwrap();
        append_timing(cache.path(), b"{}\n", &counted).unwrap();
        append_timing(cache.path(), b"{}\n", &counted).unwrap();
        let log = cache.path().join("board/timings.jsonl");
        assert_eq!(lines(&log), 2);
        let mode = std::fs::metadata(&log).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_line_that_would_pass_one_mebibyte_digests_the_log_then_starts_a_new_one() {
        let cache = tempfile::tempdir().unwrap();
        let log = cache.path().join("board/timings.jsonl");
        let previous = log.with_extension("jsonl.1");
        let half = vec![b'x'; 512 * 1024];
        append_timing(cache.path(), &half, &counted).unwrap();
        append_timing(cache.path(), &half, &counted).unwrap();
        assert!(!previous.exists(), "a log of exactly 1 MiB is kept");
        assert_eq!(read_timings(cache.path()).digest, b"");
        append_timing(cache.path(), b"\n", &counted).unwrap();
        assert_eq!(std::fs::metadata(&previous).unwrap().len(), 1024 * 1024);
        assert_eq!(std::fs::read(&log).unwrap(), b"\n");
        let timings = read_timings(cache.path());
        assert_eq!(timings.digest, b"1048576\n");
        assert_eq!(timings.current, b"\n");
        let digest = cache.path().join("board/timing-digest.jsonl");
        let mode = std::fs::metadata(digest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_run_that_finds_the_rotation_lock_held_appends_without_rotating_and_a_free_lock_rotates() {
        let cache = tempfile::tempdir().unwrap();
        let full = vec![b'x'; 1024 * 1024];
        append_timing(cache.path(), &full, &counted).unwrap();
        let held = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.path().join("board/timings.lock"))
            .unwrap();
        held.lock().unwrap();
        append_timing(cache.path(), b"\n", &counted).unwrap();
        let timings = read_timings(cache.path());
        assert_eq!(
            (timings.previous.len(), timings.current.len()),
            (0, 1024 * 1024 + 1)
        );
        held.unlock().unwrap();
        append_timing(cache.path(), b"\n", &counted).unwrap();
        let timings = read_timings(cache.path());
        assert_eq!(
            (timings.previous.len(), timings.current.len()),
            (1024 * 1024 + 1, 1)
        );
    }

    #[test]
    fn a_digest_that_cannot_be_written_still_rotates_the_log() {
        let cache = tempfile::tempdir().unwrap();
        append_timing(cache.path(), &vec![b'x'; 1024 * 1024], &counted).unwrap();
        // A directory where the digest belongs.
        std::fs::create_dir_all(cache.path().join("board/timing-digest.jsonl/x")).unwrap();
        append_timing(cache.path(), b"\n", &counted).unwrap();
        let timings = read_timings(cache.path());
        assert_eq!(
            (timings.previous.len(), timings.current),
            (1024 * 1024, b"\n".to_vec())
        );
    }

    #[test]
    fn the_stamp_is_absent_until_written_and_then_recent() {
        let cache = tempfile::tempdir().unwrap();
        assert_eq!(timing_stamped(cache.path()), None);
        stamp_timing(cache.path()).unwrap();
        let age = timing_stamped(cache.path()).unwrap().elapsed().unwrap();
        assert!(age.as_secs() < 60);
    }
}

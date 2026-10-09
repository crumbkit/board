//! The timing log (docs/architecture.md, Timing log).

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

/// The size past which the log is renamed to `timings.jsonl.1`.
const TIMING_LOG_BYTES: u64 = 1024 * 1024;

/// Appends `line` to the timing log under `cache`, which only its owner can
/// read, first renaming a log that `line` would take past 1 MiB.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn append_timing(cache: &Path, line: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(cache.join("board"))?;
    let log = cache.join("board/timings.jsonl");
    let length = std::fs::metadata(&log).map_or(0, |metadata| metadata.len());
    if length.saturating_add(line.len() as u64) > TIMING_LOG_BYTES {
        std::fs::rename(&log, log.with_extension("jsonl.1"))?;
    }
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&log)?
        .write_all(line)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn lines(path: &Path) -> usize {
        std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
    }

    #[test]
    fn a_line_is_appended_to_a_log_only_its_owner_can_read() {
        let cache = tempfile::tempdir().unwrap();
        append_timing(cache.path(), b"{}\n").unwrap();
        append_timing(cache.path(), b"{}\n").unwrap();
        let log = cache.path().join("board/timings.jsonl");
        assert_eq!(lines(&log), 2);
        let mode = std::fs::metadata(&log).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_line_that_would_pass_one_mebibyte_starts_a_new_log_and_one_that_fits_does_not() {
        let cache = tempfile::tempdir().unwrap();
        let log = cache.path().join("board/timings.jsonl");
        let previous = log.with_extension("jsonl.1");
        let half = vec![b'x'; 512 * 1024];
        append_timing(cache.path(), &half).unwrap();
        append_timing(cache.path(), &half).unwrap();
        assert!(!previous.exists(), "a log of exactly 1 MiB is kept");
        append_timing(cache.path(), b"\n").unwrap();
        assert_eq!(std::fs::metadata(&previous).unwrap().len(), 1024 * 1024);
        assert_eq!(std::fs::read(&log).unwrap(), b"\n");
    }
}

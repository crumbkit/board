//! A directory of answers that took a model to reach, each in a file named
//! by the hex of the digest its caller derives from the question. Deleting the directory
//! loses nothing but time: each answer is reached again.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::open::hex;

/// The answer kept under `digest` in `directory`, or none when there is
/// none or it cannot be read.
pub fn kept_verdict(directory: &Path, digest: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(file(directory, digest)).ok()
}

fn file(directory: &Path, digest: &[u8]) -> PathBuf {
    directory.join(format!("{}.json", hex(digest)))
}

/// Keeps `answer` under `digest` in `directory`, which it creates. The file is
/// renamed into place, so another call never reads it half written.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn keep_verdict(directory: &Path, digest: &[u8], answer: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(answer)?;
    temporary
        .persist(file(directory, digest))
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_verdict_is_read_back_and_a_missing_one_is_none() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("controls").join("judged");
        assert_eq!(kept_verdict(&directory, b"abc"), None);
        keep_verdict(&directory, b"abc", b"[]").unwrap();
        assert_eq!(kept_verdict(&directory, b"abc"), Some(b"[]".to_vec()));
        keep_verdict(&directory, b"abc", b"[1]").unwrap();
        assert_eq!(kept_verdict(&directory, b"abc"), Some(b"[1]".to_vec()));
    }
}

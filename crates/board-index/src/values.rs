//! Small values a built-in plugin caches, such as a model's verdicts, each
//! in a file named by the hex of the content hash its caller derives from
//! what the value answers. Deleting the directory loses nothing but time:
//! each value is computed again.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::open::hex;

/// The value cached under `digest` in `directory`, or none when there is
/// none or it cannot be read.
pub fn cached(directory: &Path, digest: &[u8]) -> Option<Vec<u8>> {
    std::fs::read(file(directory, digest)).ok()
}

fn file(directory: &Path, digest: &[u8]) -> PathBuf {
    directory.join(format!("{}.json", hex(digest)))
}

/// Caches `answer` under `digest` in `directory`, which it creates. The file is
/// renamed into place, so another call never reads it half written.
#[expect(
    clippy::disallowed_methods,
    reason = "board-index owns the cache's files"
)]
pub fn cache(directory: &Path, digest: &[u8], answer: &[u8]) -> io::Result<()> {
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
    fn a_cached_is_read_back_and_a_missing_one_is_none() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("controls").join("judged");
        assert_eq!(cached(&directory, b"abc"), None);
        cache(&directory, b"abc", b"[]").unwrap();
        assert_eq!(cached(&directory, b"abc"), Some(b"[]".to_vec()));
        cache(&directory, b"abc", b"[1]").unwrap();
        assert_eq!(cached(&directory, b"abc"), Some(b"[1]".to_vec()));
    }
}

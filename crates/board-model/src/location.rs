//! Where a resolution ends (docs/architecture.md, Pointers: links and mounts).

use std::fmt;
use std::path::PathBuf;

use crate::path::LogicalPath;

/// A git object id. Holding the bytes makes every value well formed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectId {
    Sha1([u8; 20]),
    Sha256([u8; 32]),
}

impl ObjectId {
    /// The id spelled by 40 or 64 lowercase hex digits.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
            .collect::<Option<_>>()?;
        if let Ok(sha1) = <[u8; 20]>::try_from(bytes.as_slice()) {
            Some(Self::Sha1(sha1))
        } else {
            <[u8; 32]>::try_from(bytes.as_slice())
                .ok()
                .map(Self::Sha256)
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Sha1(bytes) => bytes,
            Self::Sha256(bytes) => bytes,
        }
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut hex = [0u8; 64];
        let bytes = self.as_bytes();
        for (at, byte) in bytes.iter().enumerate() {
            hex[2 * at] = DIGITS[usize::from(byte >> 4)];
            hex[2 * at + 1] = DIGITS[usize::from(byte & 0xf)];
        }
        // Only ASCII digits were written.
        formatter.write_str(std::str::from_utf8(&hex[..2 * bytes.len()]).map_err(|_| fmt::Error)?)
    }
}

/// What a board is read from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// The checked-out files, uncommitted edits included.
    WorkingTree,
    /// The tree of this commit.
    Commit(ObjectId),
}

/// One repository read from one source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    /// The repository's canonical working directory.
    pub repository: PathBuf,
    pub source: Source,
}

/// What a path resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Object {
    Blob(ObjectId),
    Directory,
}

/// The end of a resolution. Its identity is its board and physical path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub board: Board,
    /// The path inside the board's tree, `/`-separated, empty for the root.
    pub path: String,
    pub object: Object,
    /// Each pointer followed to get here, in order.
    pub pointers: Vec<Location>,
}

impl Location {
    /// The path that names this Location from the root board, with every
    /// link replaced by its target and every mount point kept
    /// (docs/architecture.md, Pointers: links and mounts). A pointer is a
    /// mount exactly when what follows it is in another board, since links
    /// stay inside theirs. `None` only if a physical path is not a valid
    /// logical one, which resolution never produces.
    pub fn display_path(&self) -> Option<LogicalPath> {
        let mut segments: Vec<&str> = Vec::new();
        let next_boards = self
            .pointers
            .iter()
            .skip(1)
            .chain(std::iter::once(self))
            .map(|step| &step.board);
        for (pointer, next) in self.pointers.iter().zip(next_boards) {
            if pointer.board != *next {
                segments.extend(addressed(&pointer.path, true));
            }
        }
        segments.extend(addressed(
            &self.path,
            matches!(self.object, Object::Blob(_)),
        ));
        format!("/{}", segments.join("/")).parse().ok()
    }
}

/// The segments that address the physical `path`: a file's `.json` is
/// dropped, since records are addressed without it.
fn addressed(path: &str, file: bool) -> Vec<&str> {
    let mut segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if file && let Some(last) = segments.last_mut() {
        *last = last.strip_suffix(".json").unwrap_or(last);
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_id_displays_as_lowercase_hex() {
        let mut bytes = [0u8; 20];
        bytes[0] = 0xab;
        bytes[19] = 0x0f;
        assert_eq!(
            ObjectId::Sha1(bytes).to_string(),
            "ab0000000000000000000000000000000000000f"
        );
        assert_eq!(ObjectId::Sha256([0xff; 32]).to_string(), "f".repeat(64));
    }
}

//! Where a resolution ends (docs/architecture.md, Pointers: links and mounts).

use std::fmt;
use std::path::PathBuf;

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
        self.as_bytes()
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
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

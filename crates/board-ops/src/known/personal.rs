//! Naming the personal board in the known boards file, which `board init
//! --personal` does (docs/architecture/paths/personal.md).

use std::io;
use std::path::{Path, PathBuf};

use board_model::{Code, Fields, Refusal};

use super::{KnownBoards, against, lexical};

/// What naming a board as the personal one does to the known boards file.
#[derive(Debug)]
pub struct Naming {
    file: PathBuf,
    /// The file's new bytes, or `None` when it already names the board in
    /// `personal` and lists it in `boards`.
    rewritten: Option<Vec<u8>>,
}

impl Naming {
    /// What naming `root` does to the known boards `file`, given `read`,
    /// what reading it gave. A file naming another personal board is
    /// refused with `board::config::personal_elsewhere`, and one that is
    /// not a list of known boards with `board::config::invalid`. Every other
    /// key stays as it was.
    pub fn plan(file: &Path, read: io::Result<Vec<u8>>, root: &Path) -> Result<Self, Box<Refusal>> {
        let mut known = KnownBoards::parse(file, read)?.known;
        let same = |entry: &Path| same(&against(Some(file), entry), root);
        if let Some(other) = known.personal.as_deref().filter(|other| !same(other)) {
            let message = format!(
                "`{}` names {} as the personal board, not {}",
                file.display(),
                other.display(),
                root.display()
            );
            let hint = format!(
                "the personal board stays {}; to make this board the personal one, edit `personal` in `{}` by hand to name {}, then run init --personal again",
                other.display(),
                file.display(),
                root.display()
            );
            let code = Code::ConfigPersonalElsewhere;
            return Err(Box::new(
                Refusal::new(code, message, &Fields::default()).with_hint(hint),
            ));
        }
        let mut changed = false;
        if known.personal.is_none() {
            known.personal = Some(root.to_path_buf());
            changed = true;
        }
        if !known.boards.iter().any(|board| same(board)) {
            known.boards.push(root.to_path_buf());
            changed = true;
        }
        let rewritten = if changed {
            let mut bytes = serde_json::to_vec_pretty(&known).map_err(|error| {
                let message = format!(
                    "cannot name {} in `{}`: {error}",
                    root.display(),
                    file.display()
                );
                let hint = "JSON holds only UTF-8 paths; run init --personal in a repository whose path is UTF-8";
                let refusal = Refusal::new(Code::ConfigInvalid, message, &Fields::default());
                Box::new(refusal.with_hint(hint))
            })?;
            bytes.push(b'\n');
            Some(bytes)
        } else {
            None
        };
        Ok(Self {
            file: file.to_path_buf(),
            rewritten,
        })
    }

    /// The known boards file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The bytes a write puts in the file, if it changes it.
    pub fn rewritten(&self) -> Option<&[u8]> {
        self.rewritten.as_deref()
    }

    /// Writes the file when it changes, as `board_tree::write_outside`
    /// writes: making its directory when missing, through a temporary file
    /// beside it, truncated if a killed run left one, renamed into place.
    /// A symbolic link at the file is followed, so the link survives and its
    /// target is rewritten. Whether it wrote.
    pub fn write(&self) -> Result<bool, Box<Refusal>> {
        let Some(bytes) = &self.rewritten else {
            return Ok(false);
        };
        let target = followed(&self.file);
        let directory = target.parent().unwrap_or(Path::new("."));
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        board_tree::write_outside(directory, &name, bytes).map_err(|error| {
            let message = format!("cannot write `{}`: {error}", self.file.display());
            let hint = "make the directory writable and run init --personal again; the board and its plugins are already set up";
            let refusal = Refusal::new(Code::PutFailed, message, &Fields::default());
            Box::new(refusal.with_hint(hint))
        })?;
        Ok(true)
    }
}

/// The file a symbolic link at `file` leads to, following at most 40
/// links as the kernel does; `file` itself when it is no link.
fn followed(file: &Path) -> PathBuf {
    let mut target = file.to_path_buf();
    for _ in 0..40 {
        let Ok(link) = std::fs::read_link(&target) else {
            break;
        };
        target = target.parent().unwrap_or(Path::new(".")).join(link);
    }
    target
}

/// Whether `entry` and `root` are one directory: each with its `..` taken
/// out as text, since the file's directory may not exist yet, then
/// canonical where it can be.
fn same(entry: &Path, root: &Path) -> bool {
    let canonical = |path: &Path| {
        let path = lexical(path);
        path.canonicalize().unwrap_or(path)
    };
    canonical(entry) == canonical(root)
}

#[cfg(test)]
mod tests;

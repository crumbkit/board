//! Only the owners of signing name a signing literal: a namespace, a
//! statement header, or the program that signs or verifies
//! (docs/architecture/writes/signing.md). `lint.toml`'s `[signing]` lists
//! both, and this check reads every source file, tests included.

use std::path::Path;

use serde::Deserialize;

use crate::files::{self, FilesError, Root};
use crate::report::{Check, CheckKind, Limit, Part};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SigningConfig {
    pub(crate) literals: Vec<String>,
    /// Files, and directories whose every file, that may name a literal,
    /// relative to the root.
    pub(crate) owners: Vec<String>,
}

impl SigningConfig {
    /// Whether `relative` is an owner or lies in an owner's directory.
    pub(crate) fn owns(&self, relative: &str) -> bool {
        self.owners.iter().any(|owner| {
            relative == owner
                || relative
                    .strip_prefix(owner.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

/// The extensions of the source files this check reads anywhere.
const EXTENSIONS: [&str; 5] = ["rs", "py", "sh", "ts", "tsx"];

/// The directories whose files without an extension are scripts this check
/// reads.
const SCRIPTS: [&str; 2] = ["bin", "plugins"];

/// Whether the check reads the file at `relative`: source code, never docs,
/// controls or the vectors, which are Markdown, JSON or data.
fn scanned(relative: &Path) -> bool {
    match relative.extension() {
        Some(extension) => EXTENSIONS.iter().any(|wanted| extension == *wanted),
        None => SCRIPTS
            .iter()
            .any(|directory| relative.starts_with(directory)),
    }
}

/// Each line of a source file outside the owners that names a literal,
/// once for each literal it names.
pub(crate) fn check(root: &Root, config: &SigningConfig) -> Result<Check, FilesError> {
    let mut parts = Vec::new();
    for path in root.sources(scanned)? {
        let relative = root.display(&path).replace('\\', "/");
        if config.owns(&relative) {
            continue;
        }
        let text = files::read(&path)?;
        for (index, line) in text.lines().enumerate() {
            for literal in &config.literals {
                for _ in line.matches(literal.as_str()) {
                    let label = format!("{relative}:{} names {literal:?}", index + 1);
                    parts.push(Part::new(label, 1));
                }
            }
        }
    }
    Ok(Check::total(CheckKind::Signing, Limit::Exactly(0), parts))
}

#[cfg(test)]
mod tests;

//! The word budgets: what an agent loads at session start, the documentation
//! map, the documentation itself, and the plan.

use std::collections::{BTreeSet, VecDeque};

use crate::config::{ContextBudget, DirectoryBudget, FileBudget};
use crate::files::{self, FilesError, Root};
use crate::report::{Check, CheckKind, Limit, Part};
use crate::words;

/// How many imports deep Claude Code follows `@path`.
const MAX_IMPORT_HOPS: usize = 5;

/// The entry files and every file they import, each counted once.
pub(crate) fn context(root: &Root, budget: &ContextBudget) -> Result<Check, FilesError> {
    let mut pending = VecDeque::new();
    for entry in &budget.entry {
        pending.push_back((files::canonical(&root.path().join(entry))?, 0));
    }
    let mut seen = BTreeSet::new();
    let mut parts = Vec::new();
    while let Some((path, hops)) = pending.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = files::read(&path)?;
        parts.push(Part::new(root.display(&path), words::count(&text)));
        if hops < MAX_IMPORT_HOPS {
            for import in words::imports(&text) {
                if let Some(target) = root.import(&path, &import) {
                    pending.push_back((target, hops + 1));
                }
            }
        }
    }
    Ok(Check::total(
        CheckKind::Context,
        Limit::AtMost(budget.max_words),
        parts,
    ))
}

/// One file's words, reported as `kind`.
pub(crate) fn file(root: &Root, kind: CheckKind, budget: &FileBudget) -> Result<Check, FilesError> {
    let words = words::count(&root.read(&budget.file)?);
    Ok(Check::total(
        kind,
        Limit::AtMost(budget.max_words),
        vec![Part::new(budget.file.clone(), words)],
    ))
}

/// One check for each directory under the docs directory, itself included.
/// Each counts only the `.md` files directly inside it, so a subdirectory's
/// words never count toward its parent.
pub(crate) fn docs(root: &Root, budget: &DirectoryBudget) -> Result<Vec<Check>, FilesError> {
    let mut checks = Vec::new();
    for node in root.markdown_nodes(&budget.directory)? {
        let mut parts = Vec::new();
        for path in &node.files {
            parts.push(Part::new(
                root.display(path),
                words::count(&files::read(path)?),
            ));
        }
        checks.push(
            Check::total(CheckKind::Docs, Limit::AtMost(budget.max_words), parts)
                .in_node(root.display(&node.directory)),
        );
    }
    Ok(checks)
}

#[cfg(test)]
mod tests;

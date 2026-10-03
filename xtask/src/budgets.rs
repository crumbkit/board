//! The three documentation budgets: what an agent loads at session start, the
//! documentation map, and the documentation itself.

use std::collections::{BTreeSet, VecDeque};

use crate::config::{ContextBudget, FileBudget, GlobBudget};
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

pub(crate) fn docs_index(root: &Root, budget: &FileBudget) -> Result<Check, FilesError> {
    let words = words::count(&root.read(&budget.file)?);
    Ok(Check::total(
        CheckKind::DocsIndex,
        Limit::AtMost(budget.max_words),
        vec![Part::new(budget.file.clone(), words)],
    ))
}

pub(crate) fn docs(root: &Root, budget: &GlobBudget) -> Result<Check, FilesError> {
    let mut parts = Vec::new();
    for path in root.glob(&budget.glob)? {
        parts.push(Part::new(
            root.display(&path),
            words::count(&files::read(&path)?),
        ));
    }
    Ok(Check::total(
        CheckKind::Docs,
        Limit::AtMost(budget.max_words),
        parts,
    ))
}

#[cfg(test)]
mod tests;
